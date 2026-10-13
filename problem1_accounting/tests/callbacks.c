// SPDX-License-Identifier: GPL-2.0-only
#define P1_HOST_TEST
#include "../bpf/callbacks.bpf.c"

static struct task_struct a, b;
static int queued[2], faults;
static bool busy;

void *bpf_task_storage_get(void *map, struct task_struct *p, void *value, u64 flags)
{
    if (!p->storage && flags)
        p->storage = calloc(1, sizeof(struct ticket));
    return p->storage;
}

int bpf_task_storage_delete(void *map, struct task_struct *p)
{
    free(p->storage);
    p->storage = NULL;
    return 0;
}

s32 scx_bpf_task_cpu(const struct task_struct *p) { return p->cpu; }
s32 scx_bpf_create_dsq(u64 id, s32 node) { return 0; }
void scx_bpf_error_bstr(char *fmt, unsigned long long *data, u32 size) { faults++; }

void scx_bpf_dsq_insert(struct task_struct *p, u64 id, u64 slice, u64 flags)
{
    assert(!p->pending && !p->queued);
    p->pending = true; // Request, not insertion.
    p->dsq = id;
}

static void kernel_commit(struct task_struct *p)
{
    assert(p->pending);
    p->pending = false;
    p->queued = true;
    if (p->dsq != SCX_DSQ_GLOBAL) {
        assert(p->dsq >= DSQ_BASE && p->dsq < DSQ_BASE + 2);
        queued[p->dsq - DSQ_BASE]++;
    }
}

static void kernel_unlink(struct task_struct *p)
{
    assert(p->queued && !p->pending);
    p->queued = false;
    if (p->dsq != SCX_DSQ_GLOBAL)
        assert(queued[p->dsq - DSQ_BASE]-- > 0);
}

bool scx_bpf_dsq_move_to_local(u64 id)
{
    if (busy) return false; // Legal failure even with a nonempty queue.
    struct task_struct *tasks[] = { &a, &b };
    for (int i = 0; i < 2; i++) {
        if (tasks[i]->queued && tasks[i]->dsq == id) {
            kernel_unlink(tasks[i]);
            return true;
        }
    }
    return false;
}

static void connected(void)
{
    struct task_struct *tasks[] = { &a, &b };
    for (u8 llc = 0; llc < 2; llc++) {
        u64 owned = 0;
        for (int i = 0; i < 2; i++) {
            struct ticket *t = tasks[i]->storage;
            owned += t && t->owned && t->llc == llc;
        }
        assert(p1_count(packed, llc) == owned);
        assert(queued[llc] <= owned);
        assert(!owned || (packed & (1ULL << llc)) || (packed & (4ULL << llc)));
    }
}

static void reset(void)
{
    assert(!a.storage && !b.storage);
    a = (struct task_struct){ .cpu = 0, .pid = 42 };
    b = (struct task_struct){ .cpu = 1, .pid = 42 }; // Same PID, different task objects.
    packed = 3;
    faults = queued[0] = queued[1] = 0;
    busy = false;
    cpu_llc[0] = 0;
    cpu_llc[1] = 1;
    assert(p1_init_task(&a, NULL) == 0);
    assert(p1_init_task(&b, NULL) == 0);
}

static void cleanup(void)
{
    assert(!a.pending && !a.queued && !b.pending && !b.queued);
    p1_exit_task(&a, NULL);
    p1_exit_task(&b, NULL);
    connected();
    assert(faults == 0);
}

static void pending_and_consume_paths(void)
{
    reset();
    p1_enqueue(&a, 0);
    connected();
    assert(a.pending && queued[0] == 0 && p1_count(packed, 0) == 1);
    u64 coverage = 2;
    assert(p1_config(&coverage) == 0);
    p1_dispatch(1, NULL); // Fails while insertion is still pending.
    connected();
    assert(packed & 4);
    kernel_commit(&a);
    busy = true;
    p1_dispatch(1, NULL); // Failure is also legal while Q is nonzero.
    connected();
    assert(queued[0] == 1 && (packed & 4));
    busy = false;
    p1_dispatch(1, NULL); // Success transfers once, but does not release N.
    connected();
    assert(queued[0] == 0 && p1_count(packed, 0) == 1);
    p1_running(&a);
    p1_running(&a); // Duplicate notification does not release twice.
    p1_dispatch(1, NULL);
    connected();
    assert(p1_count(packed, 0) == 0 && !(packed & 4));
    cleanup();
}

static void reenqueue_disable_and_pid_reuse(void)
{
    reset();
    p1_enqueue(&a, 0);
    p1_enqueue(&b, 0);
    kernel_commit(&a);
    kernel_commit(&b);
    u64 coverage = 0;
    assert(p1_config(&coverage) == 0);
    connected(); // Quiescent would run here, BEFORE kernel_unlink.
    kernel_unlink(&a);
    a.cpu = 1;
    p1_enqueue(&a, 0); // Release old generation; register new queue/generation.
    connected();
    assert(((struct ticket *)a.storage)->generation == 2);
    assert(p1_count(packed, 0) == 0 && p1_count(packed, 1) == 2);
    kernel_commit(&a);
    kernel_unlink(&a);
    p1_disable(&a);
    p1_disable(&a);
    connected();
    assert(p1_count(packed, 1) == 1); // b's ticket survives a's cleanup.
    kernel_unlink(&b);
    cleanup(); // exit_task retires b's remaining generation.
}

static void invalid_config_and_untracked_fallback(void)
{
    reset();
    u64 invalid = 4;
    assert(p1_config(&invalid) == -22 && packed == 3);
    a.cpu = 255; // CPU outside configured topology.
    p1_enqueue(&a, 0);
    assert(a.pending && a.dsq == SCX_DSQ_GLOBAL);
    connected();
    kernel_commit(&a);
    kernel_unlink(&a);
    cleanup();
}

int main(void)
{
    pending_and_consume_paths();
    reenqueue_disable_and_pid_reuse();
    invalid_config_and_untracked_fallback();
    puts("actual C callbacks + actual Rust word bodies: 3 lifecycle scenarios passed (mock kernel)");
    return 0;
}
