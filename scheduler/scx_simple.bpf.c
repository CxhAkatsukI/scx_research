#include <vmlinux.h>             /* Kernel struct definitions */
#include <bpf/bpf_helpers.h>     /* BPF helper functions */
#include <bpf/bpf_tracing.h>     /* Tracking related functions */

char _license[] SEC("license") = "GPL"; /* Licence */

/* Tell the compiler about kernel functions */
extern s32 scx_bpf_create_dsq(u64 dsq_id, s32 node_id) __ksym;
extern void scx_bpf_dispatch(struct task_struct *p, u64 dsq_id, u64 slice, u64 enq_flags) __ksym;
extern void scx_bpf_consume(u64 dsq_id) __ksym;

/* define default time slice */
#ifndef SCX_SLICE_DFL
#define SCX_SLICE_DFL 20000000
#endif

/* Define the macro `SCX_OPS` */
#define SCX_OPS(name, args...) SEC("struct_ops/"#name) BPF_PROG(name, ##args)

/* Constant definition */
#define SHARED_DSQ_ID 0          /* Only one global queue */

/* Create a global queue */
s32 SCX_OPS(simple_init)
{
    // 创建 DSQ ID=0, node=-1 (不绑定NUMA节点)
    return scx_bpf_create_dsq(SHARED_DSQ_ID, -1);
}

/* Enqueue Logic */
void SCX_OPS(simple_enqueue, struct task_struct *p, u64 enq_flags)
{
    scx_bpf_dispatch(p, SHARED_DSQ_ID, SCX_SLICE_DFL, enq_flags);
}

/* Dispatch Logic */
void SCX_OPS(simple_dispatch, s32 cpu, struct task_struct *prev)
{
    /* Consume a task from the queue (FIFO) */
    scx_bpf_consume(SHARED_DSQ_ID);
}

/* Register the functions to kernel */
SEC(".struct_ops.link")
struct sched_ext_ops simple_ops = {
    .enqueue    = (void *)simple_enqueue,
    .dispatch   = (void *)simple_dispatch,
    .init       = (void *)simple_init,
    .name       = "simple_scheduler", /* Register the scheduler's name*/
};