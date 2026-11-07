// SPDX-License-Identifier: GPL-2.0
// Derive model steps from checked local receipts plus explicit primitive facts.
use crate::{*, monitor::*, model::*};
verus! {
pub enum Event { Receipt { slot: int, op: Op }, Return(int), Land(int) }
pub open spec fn apply(s: State, e: Event, fixed: bool) -> State {
    match e {
        Event::Receipt { slot, op } => next(s, slot, op, fixed),
        Event::Return(k) => State { returned: s.returned.insert(k), ..s },
        Event::Land(k) => State { landed: s.landed.insert(k), ..s },
    }
}
// Shared values/object identity, no overflow, and deferred landing are trusted.
// This predicate intentionally does NOT require allowed() or model::enabled().
pub open spec fn primitive_fact(s: State, e: Event) -> bool {
    match e {
        Event::Receipt { slot: k, op } => slot(s, k) && !s.returned.contains(k) && coherent(s, op),
        Event::Return(k) => slot(s, k) && !s.returned.contains(k),
        Event::Land(k) => s.credits.contains(k) && s.returned.contains(k) && !s.landed.contains(k),
    }
}
pub open spec fn local_fact(s: State, e: Event, fixed: bool) -> bool {
    match e {
        Event::Receipt { slot: k, op } => allowed(s.pcs[k], op, fixed),
        Event::Return(k) => s.pcs[k] == Pc::Done,
        Event::Land(_) => true,
    }
}
pub proof fn checked_receipt_enables(s: State, k: int, xs: Seq<Op>, start: Pc,
    end: Pc, i: nat, fixed: bool)
    requires follows(start, xs, end, fixed), i < xs.len(),
        s.pcs[k] == prefix(start, xs, i, fixed),
        primitive_fact(s, Event::Receipt { slot: k, op: xs[i as int] }),
    ensures enabled(s, k, xs[i as int], fixed),
{
    checked_prefix(start, xs, end, i, fixed);
}
pub proof fn unchanged_repair_debt(s: State, r: State)
    requires inv(s), shape(r), r.n == s.n, r.drain == s.drain, r.pcs == s.pcs,
    ensures inv(r),
{
    if r.n > 0 && !r.drain {
        let k = choose|k: int| #![auto] slot(s, k) && owes(s.pcs[k]);
        assert(slot(r, k) && owes(r.pcs[k]));
        assert(exists|j: int| #![auto] slot(r, j) && owes(r.pcs[j]));
    }
}
pub proof fn event_preserves(s: State, e: Event)
    requires inv(s), primitive_fact(s, e), local_fact(s, e, true),
    ensures inv(apply(s, e, true)),
{
    match e {
        Event::Receipt { slot, op } => {
            preserves(s, slot, op);
            assert(inv(next(s, slot, op, true)));
            assert(apply(s, e, true) == next(s, slot, op, true));
            assert(inv(apply(s, e, true)));
        },
        Event::Return(k) => {
            let r = apply(s, e, true);
            assert forall|j: int| #[trigger] r.returned.contains(j)
                implies slot(r, j) && r.pcs[j] == Pc::Done by {}
            assert(r.landed.subset_of(r.returned));
            assert(shape(r));
            unchanged_repair_debt(s, r);
        },
        Event::Land(k) => {
            let r = apply(s, e, true);
            assert(r.landed.subset_of(r.credits));
            assert(r.landed.subset_of(r.returned));
            assert(shape(r));
            unchanged_repair_debt(s, r);
        },
    }
    assert(inv(apply(s, e, true)));
}
pub open spec fn run(s: State, es: Seq<Event>, fixed: bool) -> State
    decreases es.len(),
{
    if es.len() == 0 { s } else {
        run(apply(s, es[0], fixed), es.subrange(1, es.len() as int), fixed)
    }
}
pub open spec fn callback_order(s: State, es: Seq<Event>, fixed: bool) -> bool
    decreases es.len(),
{
    es.len() == 0 || local_fact(s, es[0], fixed)
        && callback_order(apply(s, es[0], fixed), es.subrange(1, es.len() as int), fixed)
}
pub open spec fn kernel_execution(s: State, es: Seq<Event>, fixed: bool) -> bool
    decreases es.len(),
{
    es.len() == 0 || primitive_fact(s, es[0])
        && kernel_execution(apply(s, es[0], fixed), es.subrange(1, es.len() as int), fixed)
}

// Any finite merge, any number of participating calls. No callback is atomic.
pub proof fn callbacks_preserve(s: State, es: Seq<Event>)
    requires inv(s), callback_order(s, es, true), kernel_execution(s, es, true),
    ensures inv(run(s, es, true)),
    decreases es.len(),
{
    if es.len() > 0 {
        event_preserves(s, es[0]);
        callbacks_preserve(apply(s, es[0], true), es.subrange(1, es.len() as int));
    }
}
pub proof fn fixed_completed_trace_is_safe(kinds: Seq<bool>, es: Seq<Event>)
    requires callback_order(initial(kinds), es, true), kernel_execution(initial(kinds), es, true),
        stable(run(initial(kinds), es, true)),
    ensures !bad(run(initial(kinds), es, true)),
{
    initialize(kinds);
    callbacks_preserve(initial(kinds), es);
    stable_is_safe(run(initial(kinds), es, true));
}

pub struct Certificate { pub start: Pc, pub end: Pc, pub ops: Seq<Op> }
pub open spec fn certificate(log: &Log) -> Certificate {
    Certificate { start: log.origin(), end: log.position(), ops: log.history() }
}
// This consumes the actual postcondition exported by the verified Rust helpers.
pub proof fn certificate_from_checked_callback(log: &Log)
    requires log.wf(), log.position() == Pc::Done,
    ensures follows(certificate(log).start, certificate(log).ops, Pc::Done, log.fixed()),
{}
pub open spec fn certified(cs: Seq<Certificate>) -> bool {
    forall|k: int| #![trigger cs[k]] 0 <= k < cs.len()
        ==> follows(cs[k].start, cs[k].ops, Pc::Done, true) && cs[k].end == Pc::Done
}
pub open spec fn coupled(s: State, cs: Seq<Certificate>, used: Seq<nat>) -> bool {
    &&& cs.len() == s.pcs.len() && used.len() == cs.len()
    &&& forall|k: int| #![trigger used[k]] 0 <= k < cs.len() ==> used[k] <= cs[k].ops.len()
        && s.pcs[k] == prefix(cs[k].start, cs[k].ops, used[k], true)
}
// A merge preserves only each certificate's local order, not model::enabled().
pub open spec fn from_certificate(cs: Seq<Certificate>, used: Seq<nat>, e: Event) -> bool {
    match e {
        Event::Receipt { slot: k, op } => 0 <= k < cs.len() && used[k] < cs[k].ops.len()
            && op == cs[k].ops[used[k] as int],
        Event::Return(k) => 0 <= k < cs.len() && used[k] == cs[k].ops.len(),
        Event::Land(_) => true,
    }
}
pub open spec fn bump(used: Seq<nat>, e: Event) -> Seq<nat> {
    match e { Event::Receipt { slot: k, .. } => used.update(k, used[k] + 1), _ => used }
}
pub open spec fn merges(cs: Seq<Certificate>, used: Seq<nat>, es: Seq<Event>) -> bool
    decreases es.len(),
{
    es.len() == 0 || from_certificate(cs, used, es[0])
        && merges(cs, bump(used, es[0]), es.subrange(1, es.len() as int))
}
pub proof fn merge_event_refines(s: State, cs: Seq<Certificate>, used: Seq<nat>, e: Event)
    requires certified(cs), coupled(s, cs, used), inv(s),
        from_certificate(cs, used, e), primitive_fact(s, e),
    ensures inv(apply(s, e, true)), coupled(apply(s, e, true), cs, bump(used, e)),
{
    match e {
        Event::Receipt { slot: k, op } => {
            assert(used[k] <= cs[k].ops.len());
            checked_receipt_enables(s, k, cs[k].ops, cs[k].start, Pc::Done, used[k], true);
            prefix_next(cs[k].start, cs[k].ops, used[k], true);
        },
        Event::Return(k) => {
            assert(s.pcs[k] == prefix(cs[k].start, cs[k].ops, used[k], true));
            prefix_end(cs[k].start, cs[k].ops, Pc::Done, true);
        },
        _ => {},
    }
    assert(local_fact(s, e, true));
    event_preserves(s, e);
    let r = apply(s, e, true);
    let u = bump(used, e);
    assert forall|j: int| #![trigger u[j]] 0 <= j < cs.len()
        implies u[j] <= cs[j].ops.len() && r.pcs[j] == prefix(cs[j].start, cs[j].ops, u[j], true) by {
        assert(s.pcs[j] == prefix(cs[j].start, cs[j].ops, used[j], true));
        match e {
            Event::Receipt { slot: k, op } => {
                if j == k { prefix_next(cs[j].start, cs[j].ops, used[j], true); }
            },
            _ => {},
        }
    }
}
pub proof fn checked_merge_preserves(s: State, cs: Seq<Certificate>, used: Seq<nat>, es: Seq<Event>)
    requires inv(s), certified(cs), coupled(s, cs, used), merges(cs, used, es), kernel_execution(s, es, true),
    ensures inv(run(s, es, true)),
    decreases es.len(),
{
    if es.len() > 0 {
        merge_event_refines(s, cs, used, es[0]);
        checked_merge_preserves(apply(s, es[0], true), cs, bump(used, es[0]), es.subrange(1, es.len() as int));
    }
}
// Main connection theorem: there is no separate callback_order premise here.
pub proof fn verified_logs_completed_trace_is_safe(kinds: Seq<bool>, cs: Seq<Certificate>, es: Seq<Event>)
    requires certified(cs), cs.len() == kinds.len(),
        forall|k: int| #![trigger cs[k]] 0 <= k < kinds.len()
            ==> cs[k].start == if kinds[k] { Pc::Enq } else { Pc::Scan },
        merges(cs, Seq::new(kinds.len(), |k: int| 0nat), es),
        kernel_execution(initial(kinds), es, true), stable(run(initial(kinds), es, true)),
    ensures !bad(run(initial(kinds), es, true)),
{
    initialize(kinds);
    let used = Seq::new(kinds.len(), |k: int| 0nat);
    assert(coupled(initial(kinds), cs, used));
    checked_merge_preserves(initial(kinds), cs, used, es);
    stable_is_safe(run(initial(kinds), es, true));
}

// The old read-empty / enable / clear execution is a legal, completed trace.
pub proof fn historical_counterexample()
    ensures exists|es: Seq<Event>| #![auto] callback_order(initial(seq![true, false]), es, false)
        && kernel_execution(initial(seq![true, false]), es, false)
        && stable(run(initial(seq![true, false]), es, false))
        && bad(run(initial(seq![true, false]), es, false)),
{
    let s = initial(seq![true, false]);
    let es = seq![
        Event::Receipt { slot: 1, op: Op::Pending(0) },
        Event::Receipt { slot: 0, op: Op::Inc },
        Event::Receipt { slot: 0, op: Op::Covered(false) },
        Event::Receipt { slot: 0, op: Op::Set },
        Event::Receipt { slot: 0, op: Op::Kick },
        Event::Return(0), Event::Land(0),
        Event::Receipt { slot: 1, op: Op::Clear }, Event::Return(1),
    ];
    reveal_with_fuel(callback_order, 10);
    reveal_with_fuel(kernel_execution, 10);
    reveal_with_fuel(run, 10);
    assert(callback_order(s, es, false));
    assert(kernel_execution(s, es, false));
    assert(stable(run(s, es, false)));
    assert(bad(run(s, es, false)));
}

// A concrete repaired merge also witnesses that the main theorem is nonvacuous.
pub proof fn repaired_counterexample()
    ensures exists|cs: Seq<Certificate>, es: Seq<Event>| #![auto]
        certified(cs) && merges(cs, seq![0nat, 0nat], es)
        && kernel_execution(initial(seq![true, false]), es, true)
        && stable(run(initial(seq![true, false]), es, true))
        && run(initial(seq![true, false]), es, true).landed.len() == 1
        && !bad(run(initial(seq![true, false]), es, true)),
{
    let kinds = seq![true, false];
    let s = initial(kinds);
    let cs = seq![
        Certificate { start: Pc::Enq, end: Pc::Done,
            ops: seq![Op::Inc, Op::Covered(false), Op::Set, Op::Kick] },
        Certificate { start: Pc::Scan, end: Pc::Done,
            ops: seq![Op::Pending(0), Op::Clear, Op::Pending(1), Op::Set, Op::Kick] },
    ];
    let es = seq![
        Event::Receipt { slot: 1, op: Op::Pending(0) },
        Event::Receipt { slot: 0, op: Op::Inc },
        Event::Receipt { slot: 0, op: Op::Covered(false) },
        Event::Receipt { slot: 0, op: Op::Set },
        Event::Receipt { slot: 0, op: Op::Kick },
        Event::Return(0), Event::Land(0),
        Event::Receipt { slot: 1, op: Op::Clear },
        Event::Receipt { slot: 1, op: Op::Pending(1) },
        Event::Receipt { slot: 1, op: Op::Set },
        Event::Receipt { slot: 1, op: Op::Kick }, Event::Return(1),
    ];
    reveal_with_fuel(follows, 6);
    assert(certified(cs));
    reveal_with_fuel(merges, 13);
    reveal_with_fuel(kernel_execution, 13);
    reveal_with_fuel(run, 13);
    assert(merges(cs, seq![0nat, 0nat], es));
    assert(kernel_execution(s, es, true));
    assert(stable(run(s, es, true)));
    verified_logs_completed_trace_is_safe(kinds, cs, es);
    assert(run(s, es, true).landed.len() == 1);
}
}
