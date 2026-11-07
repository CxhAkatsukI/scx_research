/* Observation only. The workload is assigned to layer 0. */
struct p1_request {
	u64 dsq, issued, enter;
};

struct {
	__uint(type, BPF_MAP_TYPE_LRU_HASH);
	__uint(max_entries, 4096);
	__type(key, u32);
	__type(value, struct p1_request);
} p1_pending SEC(".maps");

struct {
	__uint(type, BPF_MAP_TYPE_RINGBUF);
	__uint(max_entries, 1 << 20);
} p1_events SEC(".maps");

u64 p1_lost;

/* kind, DSQ, PID, begin, end, request time, Q, C, D, task on DSQ */
static __always_inline void p1_emit(u64 *event)
{
	if (bpf_ringbuf_output(&p1_events, event, 10 * sizeof(u64), 0))
		__sync_fetch_and_add(&p1_lost, 1);
}

static __noinline void p1_requested(u32 pid, u64 dsq)
{
	struct p1_request req = { .dsq = dsq, .issued = bpf_ktime_get_ns() };
	u64 event[10] = { 1, dsq, pid, req.issued };

	if (bpf_map_update_elem(&p1_pending, &pid, &req, BPF_ANY)) {
		__sync_fetch_and_add(&p1_lost, 1);
		return;
	}
	p1_emit(event);
}

static __noinline void p1_rechecked(u64 dsq, u64 before_clear, u64 q)
{
	u64 event[10] = { 2, dsq, 0, before_clear, bpf_ktime_get_ns(), 0, q };

	p1_emit(event);
}

SEC("fentry/dispatch_enqueue")
int BPF_PROG(p1_land_enter, void *sch, struct scx_dispatch_q *dsq,
	     struct task_struct *p)
{
	u64 id = BPF_CORE_READ(dsq, id);
	if (id >= MAX_LLCS)
		return 0;

	u32 pid = BPF_CORE_READ(p, pid);
	struct p1_request *req = bpf_map_lookup_elem(&p1_pending, &pid);
	if (req && req->dsq == id)
		req->enter = bpf_ktime_get_ns();
	return 0;
}

SEC("fexit/dispatch_enqueue")
int BPF_PROG(p1_land_exit, void *sch, struct scx_dispatch_q *dsq,
	     struct task_struct *p)
{
	u64 id = BPF_CORE_READ(dsq, id);
	if (id >= MAX_LLCS)
		return 0;

	u32 pid = BPF_CORE_READ(p, pid);
	struct p1_request *req = bpf_map_lookup_elem(&p1_pending, &pid);
	if (!req || req->dsq != id || !req->enter)
		return 0;

	u32 nid = llc_node_id(id);
	if (nid >= MAX_NUMA_NODES)
		return 0;

	/* Bracket Q with C/D reads. This is still not an atomic snapshot. */
	u64 bit = 1ULL << id;
	u32 c = READ_ONCE(layers[0].nr_llc_cpus[id]);
	u64 d = READ_ONCE(layers[0].node[nid].llcs_to_drain);
	u32 q = BPF_CORE_READ(dsq, nr);
	bool on_dsq = BPF_CORE_READ(p, scx.dsq) == dsq;
	d |= READ_ONCE(layers[0].node[nid].llcs_to_drain);
	c |= READ_ONCE(layers[0].nr_llc_cpus[id]);

	u64 event[10] = {
		3, id, pid, req->enter, bpf_ktime_get_ns(), req->issued, q,
		!!c, !!(d & bit), on_dsq,
	};
	p1_emit(event);
	bpf_map_delete_elem(&p1_pending, &pid);
	return 0;
}
