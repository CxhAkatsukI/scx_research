#include <vmlinux.h>             /* Kernel struct definitions */
#include <bpf/bpf_helpers.h>     /* BPF helper functions */
#include <bpf/bpf_tracing.h>     /* Tracking related functions */

char _license[] SEC("license") = "GPL"; /* Licence */

/* Tell the compiler about kernel functions */
extern s32 scx_bpf_create_dsq(u64 dsq_id, s32 node_id) __ksym;
/* Updated for 6.18+: dispatch -> dsq_insert */
extern void scx_bpf_dsq_insert(struct task_struct *p, u64 dsq_id, u64 slice, u64 enq_flags) __ksym;
/* Updated for 6.18+: consume -> dsq_move_to_local */
extern bool scx_bpf_dsq_move_to_local(u64 dsq_id) __ksym;

/* Define time slices for different priority tiers */
#define SCX_SLICE_VIP      20000000  /* 20ms: High priority for critical tasks */
#define SCX_SLICE_NORMAL   10000000  /* 10ms: Normal response for Shell/SSH */
#define SCX_SLICE_HOG      1000000   /* 1ms:  Punishment for hogs (making it too short will cause trouble!!) */

/* Define the macro `SCX_OPS` */
#define SCX_OPS(name, args...) SEC("struct_ops/"#name) BPF_PROG(name, ##args)

/* Define the macro `SCX_OPS_SLEEPABLE` for functions that need to sleep (like init) */
#define SCX_OPS_SLEEPABLE(name, args...) SEC("struct_ops.s/"#name) BPF_PROG(name, ##args)

/* Constant definition */
#define SHARED_DSQ_ID 0          /* Only one global queue */

/* Create a global queue */
/* FIXED: Used SCX_OPS_SLEEPABLE because scx_bpf_create_dsq is a sleepable kfunc */
s32 SCX_OPS_SLEEPABLE(simple_init)
{
    return scx_bpf_create_dsq(SHARED_DSQ_ID, -1);
}

/* Helper function to check process name */
static __always_inline bool match_name(const char *comm, const char *target)
{
    for (int i = 0; i < 16; i++) {
        if (target[i] == '\0') return true; /* End of target string, match found */
        if (comm[i] != target[i]) return false; /* Mismatch */
    }
    return true;
}

/* Enqueue Logic */
void SCX_OPS(simple_enqueue, struct task_struct *p, u64 enq_flags)
{
    /* Tier 1: Critical Task (VIP) */
    if (match_name(p->comm, "critical_2")) {
        /* Insert at HEAD (jump queue) with Long Slice */
        scx_bpf_dsq_insert(p, SHARED_DSQ_ID, SCX_SLICE_VIP, enq_flags | SCX_ENQ_HEAD);
        return;
    }

    /* Tier 2: CPU Hog (Trash) */
    if (match_name(p->comm, "hog")) {
        /* Insert at TAIL with Tiny Slice (Throttle them) */
        scx_bpf_dsq_insert(p, SHARED_DSQ_ID, SCX_SLICE_HOG, enq_flags);
        return;
    }

    /* Tier 3: Everything else (SSH, Systemd, Shell) */
    /* Insert at TAIL but with a Normal Slice to keep system responsive */
    scx_bpf_dsq_insert(p, SHARED_DSQ_ID, SCX_SLICE_NORMAL, enq_flags);
}

/* Dispatch Logic */
void SCX_OPS(simple_dispatch, s32 cpu, struct task_struct *prev)
{
    /* Move task from our shared queue to the local CPU */
    scx_bpf_dsq_move_to_local(SHARED_DSQ_ID);
}

/* Visualizing Tracker */
/* Activate when a task begins to run on CPU */
void SCX_OPS(simple_running, struct task_struct *p) {
    /* Only print process whose name begins with h */
    if (p->comm[0] == 'c' || p->comm[0] == 'h') {
        s32 cpu = bpf_get_smp_processor_id();
        /* Print to kernel log */
        bpf_printk("[SCX] CPU=%d EV=START COMM=%s", cpu, p->comm);
    }
}

/* Activate when a task is stopped */
void SCX_OPS(simple_stopping, struct task_struct *p, bool runnable) {
    if (p->comm[0] == 'c' || p->comm[0] == 'h') {
        s32 cpu = bpf_get_smp_processor_id();
        bpf_printk("[SCX] CPU=%d EV=STOP COMM=%s", cpu, p->comm);
    }
}

/* Register the functions to kernel */
SEC(".struct_ops.link")
struct sched_ext_ops simple_ops = {
    .enqueue    = (void *)simple_enqueue,
    .dispatch   = (void *)simple_dispatch,
    .init       = (void *)simple_init,
    .running    = (void *)simple_running,
    .stopping   = (void *)simple_stopping,
    .name       = "simple_scheduler", /* Register the scheduler's name*/
};