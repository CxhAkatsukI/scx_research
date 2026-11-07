// SPDX-License-Identifier: GPL-2.0
// Source mapping: llc_aware.bpf.h, PR #3760. See docs/scope.md for exclusions.
use crate::{*, monitor::*, primitives::*};
verus! {
pub struct Shared {
    pub nr_queued: Counter,
    pub llcs_with_cpus: Coverage,
    pub llcs_to_drain: DrainMask,
}

// account_subcell_llc_enqueue() after successful lookups, for one selected LLC.
// The actual enqueue issues its insertion request BEFORE calling this helper.
pub fn account_enqueue(s: &Shared, log: &mut Log)
    requires old(log).wf(), old(log).position() == Pc::Enq,
    ensures final(log).wf(), final(log).position() == Pc::Done,
        final(log).origin() == old(log).origin(), final(log).fixed() == old(log).fixed(),
{
    s.nr_queued.fetch_add(log);
    if s.llcs_with_cpus.load(log) { return; }
    s.llcs_to_drain.fetch_or(log);
    kick_subcell_idle_cpu(log);
}

// Exactly the empty-count branch, not the rest of try_draining_work().
// false means control must continue into the excluded nonempty/consume branch.
pub fn empty_queue_branch(s: &Shared, fixed: bool, log: &mut Log) -> (handled: bool)
    requires old(log).wf(), old(log).position() == Pc::Scan, old(log).fixed() == fixed,
    ensures final(log).wf(), final(log).position() == Pc::Done,
        final(log).origin() == old(log).origin(), final(log).fixed() == fixed,
{
    let pending = s.nr_queued.load(log);
    if pending != 0 { return false; }
    s.llcs_to_drain.fetch_and(log);
    if fixed && s.nr_queued.load(log) != 0 {
        s.llcs_to_drain.fetch_or(log);
        kick_subcell_idle_cpu(log);
    }
    true
}
}
