#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

extern s32 rust_scheduler_init(void);
extern void rust_scheduler_enqueue(struct task_struct *p, u64 enq_flags);
extern void rust_scheduler_dispatch(void);

s32 scx_api_create_dsq(u64 dsq_id, s32 node_id)
{
	return scx_bpf_create_dsq(dsq_id, node_id);
}

void scx_api_dsq_insert(struct task_struct *p, u64 dsq_id,
			u64 slice_ns, u64 enq_flags)
{
	scx_bpf_dsq_insert(p, dsq_id, slice_ns, enq_flags);
}

bool scx_api_dsq_move_to_local(u64 dsq_id)
{
	return scx_bpf_dsq_move_to_local(dsq_id);
}

#define SCX_OPS(name, args...) SEC("struct_ops/" #name) BPF_PROG(name, ##args)
#define SCX_OPS_SLEEPABLE(name, args...) \
	SEC("struct_ops.s/" #name) BPF_PROG(name, ##args)

s32 SCX_OPS_SLEEPABLE(minimal_init)
{
	return rust_scheduler_init();
}

void SCX_OPS(minimal_enqueue, struct task_struct *p, u64 enq_flags)
{
	rust_scheduler_enqueue(p, enq_flags);
}

void SCX_OPS(minimal_dispatch, s32 cpu, struct task_struct *prev)
{
	(void)cpu;
	(void)prev;
	rust_scheduler_dispatch();
}

SEC(".struct_ops.link")
struct sched_ext_ops minimal_ops = {
	.enqueue = (void *)minimal_enqueue,
	.dispatch = (void *)minimal_dispatch,
	.init = (void *)minimal_init,
	.name = "rust_minimal",
};

char _license[] SEC("license") = "GPL";
