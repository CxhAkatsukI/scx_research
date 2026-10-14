// SPDX-License-Identifier: GPL-2.0-only
/* Bounded integration test; host loading requires explicit --allow-host. */
#define _GNU_SOURCE
#include <bpf/bpf.h>
#include <sched.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <unistd.h>

typedef uint8_t u8;
typedef uint32_t u32;
typedef int32_t i32;
typedef uint64_t u64;
#define BPF_NO_PRESERVE_ACCESS_INDEX
#include "p1.types.h"
#include "p1.skel.h"

#define WORKERS 8
#define SCHED_EXT_POLICY 7

struct progress {
    _Atomic int stop;
    _Atomic unsigned long steps[WORKERS];
};

static int state_is(const char *expected)
{
    char state[32] = {0};
    FILE *f = fopen("/sys/kernel/sched_ext/state", "r");
    if (!f)
        return 0;
    int ok = fgets(state, sizeof(state), f) && !strcmp(state, expected);
    fclose(f);
    return ok;
}

static int pin_cpu(int cpu)
{
    cpu_set_t set;
    CPU_ZERO(&set);
    CPU_SET(cpu, &set);
    return sched_setaffinity(0, sizeof(set), &set);
}

static int worker(struct progress *p, int i, pid_t parent)
{
    struct sched_param param = {0};
    /* No inherited BPF link may survive loss of the controller. */
    prctl(PR_SET_PDEATHSIG, SIGKILL);
    if (getppid() != parent || close_range(3, ~0U, 0) || pin_cpu(i % 2))
        return 1;
    if (sched_setscheduler(0, SCHED_EXT_POLICY, &param))
        return 2;
    if (sched_getscheduler(0) != SCHED_EXT_POLICY)
        return 3;
    while (!atomic_load(&p->stop)) {
        for (volatile int spin = 0; spin < 20000; spin++) { }
        unsigned long n = atomic_fetch_add(&p->steps[i], 1);
        if (i == 0 && n % 31 == 0 && pin_cpu((n / 31) % 2))
            return 4;
        if (n % 7 == 0)
            usleep(50); // Sleep/wakeup and removal before a later enqueue.
        else
            sched_yield();
    }
    /* Exercises disable before task exit, in addition to running/re-enqueue. */
    return sched_setscheduler(0, SCHED_OTHER, &param) ? 5 : 0;
}

static int config(struct p1 *s, u64 coverage)
{
    LIBBPF_OPTS(bpf_test_run_opts, opts,
        .ctx_in = &coverage,
        .ctx_size_in = sizeof(coverage),
    );
    if (bpf_prog_test_run_opts(bpf_program__fd(s->progs.p1_config), &opts))
        return -1;
    return (int)opts.retval;
}

static int safe(u64 w)
{
    return w < (1U << 18) && (!(w >> 2) || (w & 1) || (w & 2));
}

int main(int argc, char **argv)
{
    int allow_host = argc == 2 && !strcmp(argv[1], "--allow-host");
    if (argc != 1 && !allow_host) {
        fputs("Usage: vm-smoke [--allow-host]\n", stderr);
        return 1;
    }
    char vendor[32] = {0};
    FILE *f = fopen("/sys/class/dmi/id/sys_vendor", "r");
    if (!f || !fgets(vendor, sizeof(vendor), f))
        return 1;
    fclose(f);
    if ((!allow_host && strcmp(vendor, "QEMU\n")) || pin_cpu(2) ||
        sched_getscheduler(0) != SCHED_OTHER || !state_is("disabled\n")) {
        fputs("Requires CPUs 0, 1, 2, inactive sched_ext, and QEMU or --allow-host\n", stderr);
        return 1;
    }
    alarm(15); // Closing the link on process death detaches the scheduler.
    setbuf(stdout, NULL);
    struct progress *p = mmap(NULL, sizeof(*p), PROT_READ | PROT_WRITE,
                              MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    struct p1 *s = NULL;
    pid_t children[WORKERS] = {0};
    int rc = 1, created = 0, updates = 0, samples = 0, busy_samples = 0;
    int drain_samples = 0;
    if (p == MAP_FAILED)
        return 1;
    s = p1__open();
    if (!s)
        goto out;
    if (p1__load(s))
        goto out;
    s->links.p1_ops = bpf_map__attach_struct_ops(s->maps.p1_ops);
    if (libbpf_get_error(s->links.p1_ops)) {
        s->links.p1_ops = NULL;
        goto out;
    }
    puts("attached partial scheduler; only worker children opt into SCHED_EXT");
    if (!state_is("enabled\n") || config(s, 2) != -22)
        goto out;
    pid_t parent = getpid();
    for (int i = 0; i < WORKERS; i++) {
        pid_t child = fork();
        if (child < 0)
            goto out;
        if (!child)
            _exit(worker(p, i, parent));
        children[created++] = child;
    }
    for (int i = 0; i < 1000; i++) {
        int result = config(s, i % 2);
        if (result && result != -11)
            goto out;
        updates += result == 0;
        u64 w = __atomic_load_n(&s->data->P1.shared.packed.inner.v.value.__0, __ATOMIC_RELAXED);
        if (!safe(w)) {
            fprintf(stderr, "invalid packed state: %llu\n", (unsigned long long)w);
            goto out;
        }
        samples++;
        busy_samples += (w >> 2) != 0;
        drain_samples += (w & 2) != 0;
        usleep(1000);
    }
    atomic_store(&p->stop, 1);
    for (int attempt = 0; attempt < 2000; attempt++) {
        int live = 0;
        for (int i = 0; i < created; i++) {
            if (!children[i])
                continue;
            int status;
            pid_t result = waitpid(children[i], &status, WNOHANG);
            if (result < 0)
                goto out;
            if (result) {
                children[i] = 0;
                if (!WIFEXITED(status) || WEXITSTATUS(status)) {
                    fprintf(stderr, "worker %d failed: status=%d\n", i, status);
                    goto out;
                }
            } else {
                live++;
            }
        }
        if (!live)
            break;
        if (attempt == 1999)
            goto out;
        usleep(1000);
    }
    unsigned long total = 0;
    for (int i = 0; i < WORKERS; i++) {
        unsigned long n = atomic_load(&p->steps[i]);
        if (n < 10)
            goto out;
        total += n;
    }
    u64 final = __atomic_load_n(&s->data->P1.shared.packed.inner.v.value.__0, __ATOMIC_RELAXED);
    if (!safe(final) || final >> 2 || !busy_samples || !drain_samples || !updates ||
        !state_is("enabled\n"))
        goto out;
    printf("OK: workers=%d progress=%lu config_updates=%d samples=%d "
           "busy_samples=%d drain_samples=%d final_N=0\n",
           WORKERS, total, updates, samples, busy_samples, drain_samples);
    rc = 0;
out:
    atomic_store(&p->stop, 1);
    p1__destroy(s); // Detach first so a stuck worker can return to CFS.
    for (int i = 0; i < created; i++) {
        if (children[i]) {
            kill(children[i], SIGKILL);
            waitpid(children[i], NULL, 0);
        }
    }
    munmap(p, sizeof(*p));
    return rc;
}

