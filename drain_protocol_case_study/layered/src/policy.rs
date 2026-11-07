// SPDX-License-Identifier: GPL-2.0
// Translation of the extracted FIFO enqueue tail and selected drain iteration.
use crate::{*, api::Api, monitor::*};
verus! {
pub fn enqueue_tail(api: &Api, log: &mut Log)
    requires old(log).wf(), old(log).position() == Pc::Insert,
    ensures final(log).wf(), final(log).origin() == old(log).origin(), final(log).position() == Pc::Done,
{
    api.dsq_insert(log);
    if !api.nr_llc_cpus_nonzero(log) {
        api.drain_enable(log);
        api.kick_idle_node_cpu(log);
    }
}
pub fn drain_selected(api: &Api, log: &mut Log) -> bool
    requires old(log).wf(), old(log).position() == Pc::Count,
    ensures final(log).wf(), final(log).origin() == old(log).origin(), final(log).position() == Pc::Done,
{
    let mut disabled = false;
    if api.dsq_nr_queued(log) <= 1 {
        api.drain_disable(log);
        disabled = true;
    }
    let consumed = api.dsq_move_to_local(log);
    if disabled && api.dsq_nr_queued(log) > 0 {
        api.drain_enable(log);
    }
    consumed
}
}
