// SPDX-License-Identifier: GPL-2.0-only
/* Kernel-facing prototype. Its lifecycle and atomic bindings are trusted;
 * only the Rust word transformations linked below have Verus proofs. */
#ifdef P1_HOST_TEST
#include "../tests/mock_kernel.h"
#else
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

#define OP(name, args...) SEC("struct_ops/" #name) BPF_PROG(name, ##args)
#define SLEEPABLE_OP(name, args...) SEC("struct_ops.s/" #name) BPF_PROG(name, ##args)
#define P1_TOPOLOGY_CONST const
#endif
#define RETRIES 32
#define CPU_BOUND 256
#define DSQ_BASE 100

extern u64 p1_count(u64 w, u8 llc);
extern u64 p1_register(u64 w, u8 llc);
extern u64 p1_publish(u64 w, u8 coverage);
extern u64 p1_release(u64 w, u8 llc);
extern u64 p1_retire(u64 w, u8 llc);

s32 scx_bpf_create_dsq(u64 id, s32 node) __ksym;
void scx_bpf_dsq_insert(struct task_struct *p, u64 id, u64 slice, u64 flags) __ksym;
bool scx_bpf_dsq_move_to_local(u64 id) __ksym;
s32 scx_bpf_task_cpu(const struct task_struct *p) __ksym;
void scx_bpf_error_bstr(char *fmt, unsigned long long *data, u32 size) __ksym;

/* Set this immutable topology before load. 2 means outside the experiment.
 * No CPU is included by default. C describes coverage of these two groups. */
P1_TOPOLOGY_CONST volatile u8 cpu_llc[CPU_BOUND] = { [0 ... CPU_BOUND - 1] = 2 };

/* Sole mutable policy word: C, D and both N counters. No separate mask map. */
u64 packed = 3;

struct ticket {
    u64 generation;
    u8 llc;
    bool owned;
};

struct {
    __uint(type, BPF_MAP_TYPE_TASK_STORAGE);
    __uint(map_flags, BPF_F_NO_PREALLOC);
    __type(key, int);
    __type(value, struct ticket);
} tickets SEC(".maps");

static __always_inline void fault(void)
{
    char fmt[] = "problem1 accounting/lifecycle failure";
    unsigned long long data = 0;
    /* This kfunc requires a non-NULL data pointer even for zero arguments. */
    scx_bpf_error_bstr(fmt, &data, 0);
}

/* Atomic read, including when another CPU modifies the word. */
static __always_inline u64 snapshot(void)
{
    return __sync_val_compare_and_swap(&packed, 0, 0);
}

enum change { P1_REGISTER, P1_PUBLISH, P1_RELEASE, P1_RETIRE };

/* Every successful CAS is one semantic step on the same shared location.
 * Failed CAS retries from the observed value; exhaustion has no local effect. */
static __always_inline bool change_word(enum change op, u8 arg)
{
    u64 old = snapshot();
    if ((op == P1_PUBLISH && arg >= 4) || (op != P1_PUBLISH && arg >= 2))
        return false;
#pragma clang loop unroll(disable)
    for (int retry = 0; retry < RETRIES; retry++) {
        u64 next, actual;
        switch (op) {
        case P1_REGISTER:
            if (p1_count(old, arg) == 65535)
                return false;
            next = p1_register(old, arg);
            break;
        case P1_PUBLISH:
            next = p1_publish(old, arg);
            break;
        case P1_RELEASE:
            if (!p1_count(old, arg))
                return false;
            next = p1_release(old, arg);
            break;
        default:
            next = p1_retire(old, arg);
        }
        actual = __sync_val_compare_and_swap(&packed, old, next);
        if (actual == old)
            return true;
        old = actual;
    }
    return false;
}

/* Only call after this task's previous user-DSQ generation cannot land again.
 * The kernel serializes these task hooks under its rq/task lifecycle locks.
 * The record is attached to a task object, never a reusable numeric PID. */
static __always_inline bool finish_removed(struct ticket *t)
{
    if (!t || !t->owned)
        return true;
    if (t->llc >= 2 || !change_word(P1_RELEASE, t->llc)) {
        fault();
        return false; // Retain ownership; never invent a successful release.
    }
    t->owned = false;
    return true;
}

s32 SLEEPABLE_OP(p1_init_task, struct task_struct *p, struct scx_init_task_args *args)
{
    return bpf_task_storage_get(&tickets, p, 0, BPF_LOCAL_STORAGE_GET_F_CREATE) ? 0 : -12;
}

/* No select_cpu insertion path may bypass accounting for a custom DSQ. */
s32 OP(p1_select_cpu, struct task_struct *p, s32 prev_cpu, u64 flags)
{
    return prev_cpu;
}

void OP(p1_enqueue, struct task_struct *p, u64 flags)
{
    struct ticket *t = bpf_task_storage_get(&tickets, p, 0, 0);
    s32 cpu = scx_bpf_task_cpu(p);
    u8 llc;
    if (!t) {
        fault();
        goto fallback;
    }
    /* Re-enqueue follows removal of the OLD generation, not the new request. */
    if (!finish_removed(t))
        goto fallback;
    if ((u32)cpu >= CPU_BOUND)
        goto fallback;
    llc = cpu_llc[(u32)cpu];
    if (llc >= 2)
        goto fallback;
    if (t->generation == ~0ULL) {
        fault();
        goto fallback;
    }
    if (!change_word(P1_REGISTER, llc))
        goto fallback; // Full/contention: no custom-DSQ request was issued.
    t->generation++;
    t->llc = llc;
    t->owned = true;
    /* REQUEST ONLY. Keep the ticket across callback return and KernelCommit. */
    scx_bpf_dsq_insert(p, DSQ_BASE + llc, SCX_SLICE_DFL, flags);
    return;
fallback:
    scx_bpf_dsq_insert(p, SCX_DSQ_GLOBAL, SCX_SLICE_DFL, flags);
}

void OP(p1_running, struct task_struct *p)
{
    finish_removed(bpf_task_storage_get(&tickets, p, 0, 0));
}

void OP(p1_disable, struct task_struct *p)
{
    finish_removed(bpf_task_storage_get(&tickets, p, 0, 0));
}

void OP(p1_exit_task, struct task_struct *p, struct scx_exit_task_args *args)
{
    struct ticket *t = bpf_task_storage_get(&tickets, p, 0, 0);
    if (finish_removed(t))
        bpf_task_storage_delete(&tickets, p);
}

/* dequeue/quiescent intentionally retain the ticket: they precede unlink.
 * A successful move identifies no task to this API and is not a release. */
void OP(p1_dispatch, s32 cpu, struct task_struct *prev)
{
    u8 own;
    u64 w;
    if ((u32)cpu >= CPU_BOUND)
        return;
    own = cpu_llc[(u32)cpu];
    if (own >= 2)
        return;
    w = snapshot();
    for (u8 llc = 0; llc < 2; llc++) {
        bool moved;
        if (!((llc == own && (w & (1ULL << llc))) || (w & (4ULL << llc))))
            continue;
        moved = scx_bpf_dsq_move_to_local(DSQ_BASE + llc);
        /* N, not a move outcome, governs drain retirement. Retry exhaustion
         * leaves the old drain state intact, so retirement can be retried. */
        change_word(P1_RETIRE, llc);
        if (moved)
            return;
    }
}

/* Execute via BPF_PROG_TEST_RUN, ctx_in = one u64 coverage mask (0..3).
 * This is the only supported configuration write path. */
SEC("syscall")
int p1_config(u64 *coverage)
{
    if (*coverage >= 4)
        return -22;
    return change_word(P1_PUBLISH, *coverage) ? 0 : -11;
}

s32 SLEEPABLE_OP(p1_init)
{
    s32 err = scx_bpf_create_dsq(DSQ_BASE, -1);
    return err ? err : scx_bpf_create_dsq(DSQ_BASE + 1, -1);
}

SEC(".struct_ops.link")
struct sched_ext_ops p1_ops = {
    .select_cpu = (void *)p1_select_cpu,
    .enqueue = (void *)p1_enqueue,
    .dispatch = (void *)p1_dispatch,
    .running = (void *)p1_running,
    .disable = (void *)p1_disable,
    .init_task = (void *)p1_init_task,
    .exit_task = (void *)p1_exit_task,
    .init = (void *)p1_init,
    .flags = SCX_OPS_SWITCH_PARTIAL,
    .name = "p1_accounting",
};

char LICENSE[] SEC("license") = "GPL";
