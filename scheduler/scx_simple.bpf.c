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

/* define default time slice */
#ifndef SCX_SLICE_DFL
#define SCX_SLICE_DFL 20000000
#endif

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

/* Enqueue Logic */
void SCX_OPS(simple_enqueue, struct task_struct *p, u64 enq_flags)
{
    scx_bpf_dsq_insert(p, SHARED_DSQ_ID, SCX_SLICE_DFL, enq_flags);
}

/* Dispatch Logic */
void SCX_OPS(simple_dispatch, s32 cpu, struct task_struct *prev)
{
    /* Consume a task from the queue (FIFO) */
    scx_bpf_dsq_move_to_local(SHARED_DSQ_ID);
}

/* Register the functions to kernel */
SEC(".struct_ops.link")
struct sched_ext_ops simple_ops = {
    .enqueue    = (void *)simple_enqueue,
    .dispatch   = (void *)simple_dispatch,
    .init       = (void *)simple_init,
    .name       = "simple_scheduler", /* Register the scheduler's name*/
};