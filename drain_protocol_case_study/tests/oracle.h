// SPDX-License-Identifier: GPL-2.0
// Host shims only: success-path lookups and instrumented atomic primitives.
#include <stdbool.h>
#include <stdint.h>
#include <errno.h>
typedef uint32_t u32;
typedef uint64_t u64;
#define MAX_LLCS 1
#define nr_llc 1u
#define enable_llc_awareness true
#define scx_bpf_error(...) ((void)0)

extern void case_before(uint8_t kind);
extern void case_after(uint8_t kind, uint32_t value);
struct subcell_llc { u32 nr_queued; };
struct subcell { u64 llcs_with_cpus, llcs_to_drain; struct subcell_llc llcs[1]; };
static struct subcell shared;
static inline struct subcell *lookup_subcell(u32 cell, u32 subcell)
{ return cell == 0 && subcell == 0 ? &shared : 0; }
static inline struct subcell_llc *lookup_subcell_llc(struct subcell *s, u32 llc)
{ return llc == 0 ? &s->llcs[0] : 0; }
static inline bool llc_is_valid(u32 llc) { return llc == 0; }
static inline void kick_subcell_idle_cpu(u32 cell, u32 subcell)
{ (void)cell; (void)subcell; case_before(5); case_after(5, 0); }

static u32 read_count(u32 *p)
{ case_before(2); u32 n = __atomic_load_n(p, __ATOMIC_RELAXED); case_after(2, n); return n; }
static u64 read_coverage(u64 *p)
{ case_before(1); u64 c = __atomic_load_n(p, __ATOMIC_RELAXED); case_after(1, c & 1); return c; }
#define READ_ONCE(x) _Generic(&(x), u32 *: read_count, u64 *: read_coverage)(&(x))
static u32 add_count(u32 *p, u32 n)
{ case_before(0); u32 old = __atomic_fetch_add(p, n, __ATOMIC_SEQ_CST); case_after(0, 0); return old; }
static u64 set_mask(u64 *p, u64 bits)
{ case_before(3); u64 n = __atomic_or_fetch(p, bits, __ATOMIC_SEQ_CST); case_after(3, 0); return n; }
static u64 clear_mask(u64 *p, u64 bits)
{ case_before(4); u64 n = __atomic_and_fetch(p, bits, __ATOMIC_SEQ_CST); case_after(4, 0); return n; }
#define __sync_fetch_and_add(p, n) add_count(p, n)
#define __sync_or_and_fetch(p, bits) set_mask(p, bits)
#define __sync_and_and_fetch(p, bits) clear_mask(p, bits)

static inline int account_subcell_llc_enqueue(u32 cell, u32 subcell, u32 llc);
static inline bool source_empty_queue_branch(void);
#define JOIN_(a, b) a##b
#define JOIN(a, b) JOIN_(a, b)
#define NAME(suffix) JOIN(CASE, suffix)
void NAME(_reset)(void) { shared = (struct subcell){.llcs_to_drain = 1}; }
void NAME(_enqueue)(void) { account_subcell_llc_enqueue(0, 0, 0); }
void NAME(_retire)(void) { source_empty_queue_branch(); }
u32 NAME(_count)(void) { return shared.llcs[0].nr_queued; }
bool NAME(_drain)(void) { return (shared.llcs_to_drain & 1) != 0; }
