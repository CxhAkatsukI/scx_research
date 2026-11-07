// SPDX-License-Identifier: GPL-2.0
// One enqueue and one already-selected drain iteration. C stays false.
use crate::{*, monitor::*};
verus! {
pub struct State {
    pub q: nat, pub d: bool, pub epc: Pc, pub rpc: Pc,
    pub returned: bool, pub landed: bool,
}
#[derive(PartialEq, Eq)]
pub enum Event { E(Op), R(Op), Return, Land }
pub open spec fn initial() -> State {
    State { q: 1, d: true, epc: Pc::Insert, rpc: Pc::Count, returned: false, landed: false }
}
// Coherence/lifecycle, not callback order. Failed consume is allowed even if Q>0.
pub open spec fn kernel_fact(s: State, e: Event) -> bool {
    match e {
        Event::E(o) | Event::R(o) => match o {
            Op::Covered(c) => !c,
            Op::Queued(n) => n == s.q,
            Op::Consume(ok) => ok ==> s.q > 0,
            _ => true,
        },
        Event::Return => !s.returned,
        Event::Land => s.returned && !s.landed,
    }
}
pub open spec fn local_order(s: State, e: Event) -> bool {
    match e {
        Event::E(o) => !s.returned && allowed(s.epc, o),
        Event::R(o) => allowed(s.rpc, o),
        Event::Return => s.epc == Pc::Done,
        Event::Land => true,
    }
}
pub open spec fn enabled(s: State, e: Event) -> bool { kernel_fact(s, e) && local_order(s, e) }
pub open spec fn next(s: State, e: Event, deferred: bool) -> State {
    match e {
        Event::E(o) => State {
            q: if o == Op::Insert && !deferred { s.q + 1 } else { s.q },
            d: if o == Op::Enable { true } else { s.d }, epc: advance(s.epc, o), ..s },
        Event::R(o) => State {
            q: if o == Op::Consume(true) { (s.q - 1) as nat } else { s.q },
            d: if o == Op::Enable { true } else if o == Op::Disable { false } else { s.d },
            rpc: advance(s.rpc, o), ..s },
        Event::Return => State { returned: true, ..s },
        Event::Land => State { q: if deferred { s.q + 1 } else { s.q }, landed: true, ..s },
    }
}
pub open spec fn completed(s: State) -> bool { s.epc == Pc::Done && s.rpc == Pc::Done && s.returned && s.landed }
pub open spec fn bad(s: State) -> bool { s.q > 0 && !s.d }
pub open spec fn legal(s: State, xs: Seq<Event>, deferred: bool) -> bool
    decreases xs.len(),
{
    xs.len() == 0 || enabled(s, xs[0]) && legal(next(s, xs[0], deferred), xs.subrange(1, xs.len() as int), deferred)
}
pub open spec fn run(s: State, xs: Seq<Event>, deferred: bool) -> State
    decreases xs.len(),
{
    if xs.len() == 0 { s } else { run(next(s, xs[0], deferred), xs.subrange(1, xs.len() as int), deferred) }
}
// This invariant is only valid for the stronger, hypothetical immediate API.
pub open spec fn immediate_inv(s: State) -> bool {
    bad(s) ==> s.epc == Pc::Coverage || s.epc == Pc::Enable
        || s.rpc == Pc::ConsumeLast || s.rpc == Pc::Recheck || s.rpc == Pc::Repair
}
pub proof fn immediate_step(s: State, e: Event)
    requires immediate_inv(s), enabled(s, e),
    ensures immediate_inv(next(s, e, false)),
{ }
pub proof fn immediate_trace(s: State, xs: Seq<Event>)
    requires immediate_inv(s), legal(s, xs, false),
    ensures immediate_inv(run(s, xs, false)),
    decreases xs.len(),
{
    if xs.len() > 0 {
        immediate_step(s, xs[0]);
        immediate_trace(next(s, xs[0], false), xs.subrange(1, xs.len() as int));
    }
}
pub proof fn immediate_completed_is_safe(xs: Seq<Event>)
    requires legal(initial(), xs, false), completed(run(initial(), xs, false)),
    ensures !bad(run(initial(), xs, false)),
{ immediate_trace(initial(), xs); }

pub open spec fn witness() -> Seq<Event> {
    seq![Event::E(Op::Insert), Event::E(Op::Covered(false)), Event::E(Op::Enable), Event::E(Op::Kick),
         Event::R(Op::Queued(1)), Event::R(Op::Disable), Event::R(Op::Consume(true)), Event::R(Op::Queued(0)),
         Event::Return, Event::Land]
}
pub proof fn deferred_counterexample()
    ensures legal(initial(), witness(), true), completed(run(initial(), witness(), true)),
        bad(run(initial(), witness(), true)), run(initial(), witness(), true).q == 1,
{
    reveal_with_fuel(legal, 12);
    reveal_with_fuel(run, 12);
}
}
