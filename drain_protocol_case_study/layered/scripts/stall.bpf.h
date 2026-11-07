/* Observation plus enqueue-to-running correlation for the stall experiment. */
struct p1_request {
	u64 dsq, issued, enter;
};

struct p1_clear {
	u64 before, after;
};

struct p1_strand {
	u64 dsq, landed, issued;
};

struct {
	__uint(type, BPF_MAP_TYPE_LRU_HASH);
	__uint(max_entries, 4096);
	__type(key, u32);
	__type(value, struct p1_request);
} p1_pending SEC(".maps");

struct {
	__uint(type, BPF_MAP_TYPE_LRU_HASH);
	__uint(max_entries, 64);
	__type(key, u64);
	__type(value, struct p1_clear);
} p1_empty SEC(".maps");

struct {
	__uint(type, BPF_MAP_TYPE_LRU_HASH);
	__uint(max_entries, 4096);
	__type(key, u32);
	__type(value, struct p1_strand);
} p1_stranded SEC(".maps");

struct {
	__uint(type, BPF_MAP_TYPE_RINGBUF);
	__uint(max_entries, 1 << 20);
} p1_events SEC(".maps");

u64 p1_lost;
u64 p1_freeze;

/* kind, DSQ, PID, begin, end, request time, Q, C, D, task on DSQ */
static __always_inline void p1_emit(u64 *event)
{
	if (bpf_ringbuf_output(&p1_events, event, 10 * sizeof(u64), 0))
		__sync_fetch_and_add(&p1_lost, 1);
}

static __noinline void p1_requested(struct task_struct *p, u64 dsq)
{
	/* Ignore the short-lived load generators; track only the long-lived target. */
	if (bpf_strncmp(p->comm, 12, "stall-target"))
		return;

	u32 pid = BPF_CORE_READ(p, pid);
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
	u64 now = bpf_ktime_get_ns();
	u64 event[10] = { 2, dsq, 0, before_clear, now, 0, q };

	if (!q) {
		struct p1_clear clear = { .before = before_clear, .after = now };
		if (bpf_map_update_elem(&p1_empty, &dsq, &clear, BPF_ANY))
			__sync_fetch_and_add(&p1_lost, 1);
	}
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
	u64 now = bpf_ktime_get_ns();

	u64 event[10] = {
		3, id, pid, req->enter, now, req->issued, q,
		!!c, !!(d & bit), on_dsq,
	};
	p1_emit(event);

	struct p1_clear *clear = bpf_map_lookup_elem(&p1_empty, &id);
	if (q > 0 && !c && !(d & bit) && on_dsq && clear &&
	    req->issued < clear->before && clear->before < clear->after &&
	    clear->after < req->enter) {
		struct p1_strand strand = {
			.dsq = id, .landed = now, .issued = req->issued,
		};
		u64 match[10] = {
			4, id, pid, req->enter, now, req->issued, q, 0, 0, on_dsq,
		};
		/* Stop later CPU-allocation updates from masking the stranded state. */
		WRITE_ONCE(p1_freeze, 1);
		if (bpf_map_update_elem(&p1_stranded, &pid, &strand, BPF_ANY))
			__sync_fetch_and_add(&p1_lost, 1);
		p1_emit(match);
	}

	bpf_map_delete_elem(&p1_pending, &pid);
	return 0;
}

static __noinline void p1_running(struct task_struct *p)
{
	u32 pid = BPF_CORE_READ(p, pid);
	struct p1_strand *strand = bpf_map_lookup_elem(&p1_stranded, &pid);
	if (!strand)
		return;

	u64 now = bpf_ktime_get_ns();
	u64 event[10] = {
		5, strand->dsq, pid, strand->landed, now, strand->issued,
	};
	p1_emit(event);
	bpf_map_delete_elem(&p1_stranded, &pid);
}
