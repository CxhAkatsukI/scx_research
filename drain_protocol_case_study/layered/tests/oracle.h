// SPDX-License-Identifier: GPL-2.0
// Successful lookup/topology shims; kernel behavior is modeled, not extracted.
#include <stdbool.h>
#include <stdint.h>
typedef uint32_t u32;
typedef uint64_t u64;
#define MAX_NUMA_NODES 1
#define lstat_inc(...) ((void)0)
extern void layered_before(uint8_t kind);
extern void layered_after(uint8_t kind, uint32_t value);
struct layer_node { u64 llcs_to_drain; };
struct layer { struct layer_node node[1]; u32 nr_llc_cpus[1]; bool fifo; u64 slice_ns; };
struct task_ctx { u64 dsq_id; };
static struct layer shared;
static struct task_ctx task;
static u32 queued;
static bool deferred;
static u32 llc_node_id(u32 llc) { (void)llc; return 0; }

static u32 read_coverage(struct layer *layer, u32 llc)
{
    layered_before(1);
    u32 n = __atomic_load_n(&layer->nr_llc_cpus[llc], __ATOMIC_SEQ_CST);
    layered_after(1, n);
    return n;
}
static u64 set_mask(u64 *p, u64 bits)
{
    layered_before(3); u64 n = __atomic_or_fetch(p, bits, __ATOMIC_SEQ_CST);
    layered_after(3, 0); return n;
}
static u64 clear_mask(u64 *p, u64 bits)
{
    layered_before(4); u64 n = __atomic_and_fetch(p, bits, __ATOMIC_SEQ_CST);
    layered_after(4, 0); return n;
}
#define __sync_or_and_fetch(p, b) set_mask(p, b)
#define __sync_and_and_fetch(p, b) clear_mask(p, b)
static void scx_bpf_dsq_insert(void *p, u64 id, u64 slice, u64 flags)
{
    (void)p; (void)id; (void)slice; (void)flags;
    layered_before(0);
    if (!deferred) __atomic_fetch_add(&queued, 1, __ATOMIC_SEQ_CST);
    layered_after(0, 0);
}
static void scx_bpf_dsq_insert_vtime(void *p, u64 id, u64 slice, u64 vtime, u64 flags)
{ (void)vtime; scx_bpf_dsq_insert(p, id, slice, flags); }
static u32 scx_bpf_dsq_nr_queued(u64 id)
{
    (void)id; layered_before(2);
    u32 n = __atomic_load_n(&queued, __ATOMIC_SEQ_CST);
    layered_after(2, n); return n;
}
static bool scx_bpf_dsq_move_to_local(u64 id, u64 flags)
{
    (void)id; (void)flags; layered_before(5);
    u32 n = __atomic_load_n(&queued, __ATOMIC_SEQ_CST);
    if (n) __atomic_fetch_sub(&queued, 1, __ATOMIC_SEQ_CST);
    layered_after(5, !!n); return !!n;
}
static void layer_kick_idle_node_cpu(struct layer *layer, u32 node)
{ (void)layer; (void)node; layered_before(6); layered_after(6, 0); }
void source_reset(bool mode)
{
    shared = (struct layer){.fifo = true, .node = {{.llcs_to_drain = 1}}};
    task = (struct task_ctx){0}; queued = 1; deferred = mode;
}
void source_finish(void)
{
    layered_before(7); layered_after(7, 0);
    layered_before(8);
    if (deferred) __atomic_fetch_add(&queued, 1, __ATOMIC_SEQ_CST);
    layered_after(8, 0);
}
u32 source_count(void) { return queued; }
bool source_enabled(void) { return (shared.node[0].llcs_to_drain & 1) != 0; }
