// SPDX-License-Identifier: GPL-2.0
// Derive legal model steps from checked Rust receipts, not a legal-trace premise.
use crate::{*, monitor::*, model::*};
verus! {
pub open spec fn certified(e: Seq<Op>, r: Seq<Op>) -> bool {
    follows(Pc::Insert, e, Pc::Done) && follows(Pc::Count, r, Pc::Done)
}
pub open spec fn coupled(s: State, e: Seq<Op>, r: Seq<Op>, ei: nat, ri: nat) -> bool {
    ei <= e.len() && ri <= r.len()
        && s.epc == prefix(Pc::Insert, e, ei) && s.rpc == prefix(Pc::Count, r, ri)
}
pub open spec fn from_log(e: Seq<Op>, r: Seq<Op>, ei: nat, ri: nat, event: Event) -> bool {
    match event {
        Event::E(o) => ei < e.len() && o == e[ei as int],
        Event::R(o) => ri < r.len() && o == r[ri as int],
        Event::Return => ei == e.len(),
        Event::Land => true,
    }
}
pub open spec fn bump_e(ei: nat, event: Event) -> nat { if event is E { ei + 1 } else { ei } }
pub open spec fn bump_r(ri: nat, event: Event) -> nat { if event is R { ri + 1 } else { ri } }
pub open spec fn merges(e: Seq<Op>, r: Seq<Op>, ei: nat, ri: nat, xs: Seq<Event>) -> bool
    decreases xs.len(),
{
    xs.len() == 0 || from_log(e, r, ei, ri, xs[0])
        && merges(e, r, bump_e(ei, xs[0]), bump_r(ri, xs[0]), xs.subrange(1, xs.len() as int))
}
// Only shared-value and kernel-lifecycle facts; callback order is not assumed.
pub open spec fn kernel_execution(s: State, xs: Seq<Event>, deferred: bool) -> bool
    decreases xs.len(),
{
    xs.len() == 0 || kernel_fact(s, xs[0])
        && kernel_execution(next(s, xs[0], deferred), xs.subrange(1, xs.len() as int), deferred)
}
pub proof fn receipt_enables(s: State, e: Seq<Op>, r: Seq<Op>, ei: nat, ri: nat,
    event: Event, deferred: bool)
    requires certified(e, r), coupled(s, e, r, ei, ri), from_log(e, r, ei, ri, event),
        kernel_fact(s, event),
        // Same callback cannot perform more operations after its return.
        event is E ==> !s.returned,
    ensures enabled(s, event),
        coupled(next(s, event, deferred), e, r, bump_e(ei, event), bump_r(ri, event)),
{
    match event {
        Event::E(_) => { prefix_step(Pc::Insert, e, Pc::Done, ei); },
        Event::R(_) => { prefix_step(Pc::Count, r, Pc::Done, ri); },
        Event::Return => { prefix_end(Pc::Insert, e, Pc::Done); },
        _ => {},
    }
}
// The return frontier excludes further enqueue instructions, by ordinary control flow.
pub open spec fn lifecycle(s: State, e: Seq<Op>, ei: nat) -> bool {
    s.returned ==> ei == e.len()
}
pub proof fn checked_merge_is_legal(s: State, e: Seq<Op>, r: Seq<Op>, ei: nat, ri: nat,
    xs: Seq<Event>, deferred: bool)
    requires certified(e, r), coupled(s, e, r, ei, ri), lifecycle(s, e, ei),
        merges(e, r, ei, ri, xs), kernel_execution(s, xs, deferred),
    ensures legal(s, xs, deferred),
    decreases xs.len(),
{
    if xs.len() > 0 {
        receipt_enables(s, e, r, ei, ri, xs[0], deferred);
        checked_merge_is_legal(next(s, xs[0], deferred), e, r,
            bump_e(ei, xs[0]), bump_r(ri, xs[0]), xs.subrange(1, xs.len() as int), deferred);
    }
}
// Actual callback postconditions export these certificates, without trusting the bodies.
pub proof fn checked_logs(log_e: &Log, log_r: &Log)
    requires log_e.wf(), log_e.origin() == Pc::Insert, log_e.position() == Pc::Done,
        log_r.wf(), log_r.origin() == Pc::Count, log_r.position() == Pc::Done,
    ensures certified(log_e.history(), log_r.history()),
{}
pub proof fn certified_histories_refine(e: Seq<Op>, r: Seq<Op>, xs: Seq<Event>, deferred: bool)
    requires certified(e, r), merges(e, r, 0, 0, xs), kernel_execution(initial(), xs, deferred),
    ensures legal(initial(), xs, deferred),
{
    checked_merge_is_legal(initial(), e, r, 0, 0, xs, deferred);
}
// Directly consume the postconditions of enqueue_tail()/drain_selected().
pub proof fn verified_callbacks_refine(log_e: &Log, log_r: &Log, xs: Seq<Event>, deferred: bool)
    requires log_e.wf(), log_e.origin() == Pc::Insert, log_e.position() == Pc::Done,
        log_r.wf(), log_r.origin() == Pc::Count, log_r.position() == Pc::Done,
        merges(log_e.history(), log_r.history(), 0, 0, xs),
        kernel_execution(initial(), xs, deferred),
    ensures legal(initial(), xs, deferred),
{
    checked_logs(log_e, log_r);
    certified_histories_refine(log_e.history(), log_r.history(), xs, deferred);
}
pub proof fn immediate_callbacks_safe(e: Seq<Op>, r: Seq<Op>, xs: Seq<Event>)
    requires certified(e, r), merges(e, r, 0, 0, xs), kernel_execution(initial(), xs, false),
        completed(run(initial(), xs, false)),
    ensures !bad(run(initial(), xs, false)),
{
    certified_histories_refine(e, r, xs, false);
    immediate_completed_is_safe(xs);
}
// A legal counterexample also belongs to the callbacks' checked operation language.
pub proof fn connected_counterexample()
    ensures certified(seq![Op::Insert, Op::Covered(false), Op::Enable, Op::Kick],
            seq![Op::Queued(1), Op::Disable, Op::Consume(true), Op::Queued(0)]),
        merges(seq![Op::Insert, Op::Covered(false), Op::Enable, Op::Kick],
            seq![Op::Queued(1), Op::Disable, Op::Consume(true), Op::Queued(0)], 0, 0, witness()),
        kernel_execution(initial(), witness(), true),
        completed(run(initial(), witness(), true)), bad(run(initial(), witness(), true)),
{
    reveal_with_fuel(follows, 6);
    reveal_with_fuel(merges, 12);
    reveal_with_fuel(kernel_execution, 12);
    reveal_with_fuel(run, 12);
    deferred_counterexample();
}
}
