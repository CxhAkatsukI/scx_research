// SPDX-License-Identifier: GPL-2.0
// Projection to the queue protocol; implementation-only ticket writes stutter.
use vstd::prelude::*;
use crate::model::*;
use crate::word::*;
verus! {
pub struct Queue { pub registered: int, pub queued: int, pub covered: bool, pub drain: bool }
pub open spec fn project(s: State) -> Queue {
    Queue { registered: n(s.word), queued: qtotal(s.tasks), covered: c(s.word), drain: d(s.word) }
}
pub open spec fn queue_inv(q: Queue) -> bool {
    0 <= q.queued <= q.registered && (q.registered > 0 && !q.covered ==> q.drain)
}
pub proof fn linked(s: State)
    requires inv(s),
    ensures queue_inv(project(s)),
{ totals(s.tasks); }

pub proof fn no_stranding(s: State)
    requires inv(s), project(s).queued > 0, !project(s).covered,
    ensures project(s).drain,
{ linked(s); }

pub open spec fn legal_trace(s: State, xs: Seq<Step>) -> bool
    decreases xs.len(),
{
    xs.len() == 0 || (enabled(s, xs[0])
        && legal_trace(next(s, xs[0]), xs.subrange(1, xs.len() as int)))
}
pub open spec fn run(s: State, xs: Seq<Step>) -> State
    decreases xs.len(),
{
    if xs.len() == 0 { s } else { run(next(s, xs[0]), xs.subrange(1, xs.len() as int)) }
}
pub proof fn every_interleaving(s: State, xs: Seq<Step>)
    requires inv(s), legal_trace(s, xs),
    ensures inv(run(s, xs)), queue_inv(project(run(s, xs))),
        project(run(s, xs)).queued > 0 && !project(run(s, xs)).covered ==> project(run(s, xs)).drain,
    decreases xs.len(),
{
    if xs.len() > 0 {
        preserves(s, xs[0]);
        every_interleaving(next(s, xs[0]), xs.subrange(1, xs.len() as int));
    } else { linked(s); }
}
}
