// SPDX-License-Identifier: GPL-2.0
// Connect verified callback receipts to shared steps. The environment supplies
// only primitive coherence and same-task lifecycle exclusion, not model::enabled.
use vstd::prelude::*;
use crate::{*, model::*, receipts::*, word::*, connection::*};
verus! {
pub struct Call { pub task: int, pub pc: Pc, pub active: bool }
pub struct System { pub shared: State, pub calls: Seq<Call> }

pub open spec fn phase_matches(pc: Pc, p: Phase) -> bool {
    match pc {
        Pc::Start => p == Phase::Off || p == Phase::Detached,
        Pc::Old => p == Phase::Detached,
        Pc::Empty => p == Phase::Off,
        Pc::Reserved => p == Phase::Reserved,
        Pc::Owned => p == Phase::Owned,
        Pc::Releasing => p == Phase::Releasing,
        Pc::Done => p == Phase::Off || p == Phase::Requested
            || p == Phase::Queued || p == Phase::Detached,
    }
}
pub open spec fn coupled(s: System, k: int) -> bool {
    let call = s.calls[k];
    !call.active || if call.task == -1 { call.pc == Pc::Start } else {
        0 <= call.task < s.shared.tasks.len() && phase_matches(call.pc, s.shared.tasks[call.task])
    }
}
pub open spec fn system_inv(s: System) -> bool {
    inv(s.shared)
    && (forall|k: int| 0 <= k < s.calls.len() ==> coupled(s, k))
    && (forall|i: int, j: int| #![auto] 0 <= i < s.calls.len() && 0 <= j < s.calls.len()
        && s.calls[i].active && s.calls[j].active && s.calls[i].task >= 0
        && s.calls[i].task == s.calls[j].task ==> i == j)
}

pub proof fn initialize(tasks: Seq<Phase>, calls: Seq<Call>)
    requires
        forall|i: int| 0 <= i < tasks.len() ==> tasks[i] == Phase::Off,
        forall|k: int| #![auto] 0 <= k < calls.len() ==> !calls[k].active,
    ensures system_inv(System { shared: State { word: 1, tasks }, calls }),
{
    initial(tasks);
}

pub enum Event {
    Begin { slot: int, task: int },
    End { slot: int },
    Receipt { slot: int, op: Op },
    KernelLand { task: int },
    KernelRemove { task: int },
    KernelCancel { task: int },
}
pub open spec fn active(s: System, k: int) -> bool { 0 <= k < s.calls.len() && s.calls[k].active }

// A raw API receipt is matched to its real object/value at its linearization point.
// Nothing here requires the scheduler's high-level transition to be enabled.
pub open spec fn primitive_coherent(s: System, k: int, o: Op) -> bool {
    let t = s.calls[k].task;
    match o {
        Op::Cas { before, observed, .. } => observed == s.shared.word,
        Op::TicketRead { owned } => t >= 0 && owned == (s.shared.tasks[t] == Phase::Detached),
        Op::TicketWrite { .. } | Op::Insert { .. } => t >= 0,
        Op::Move { .. } => t == -1,
    }
}
pub open spec fn permitted(s: System, e: Event) -> bool {
    match e {
        Event::Begin { slot, task } => 0 <= slot < s.calls.len() && !s.calls[slot].active
            && (task == -1 || (0 <= task < s.shared.tasks.len()
                && (s.shared.tasks[task] == Phase::Off || s.shared.tasks[task] == Phase::Detached)))
            && (forall|j: int| #![auto] 0 <= j < s.calls.len() && s.calls[j].active && task >= 0
                ==> s.calls[j].task != task),
        Event::End { slot } => active(s, slot) && (s.calls[slot].pc == Pc::Start
            || s.calls[slot].pc == Pc::Old || s.calls[slot].pc == Pc::Empty || s.calls[slot].pc == Pc::Done),
        Event::Receipt { slot, op } => active(s, slot) && allowed(s.calls[slot].pc, op)
            && cas_sound(op) && primitive_coherent(s, slot, op),
        Event::KernelLand { task } => at(s.shared, task, Phase::Requested),
        Event::KernelRemove { task } => at(s.shared, task, Phase::Queued),
        Event::KernelCancel { task } => at(s.shared, task, Phase::Requested),
    }
}

// Separate what verified callback bodies establish from the external execution
// premises. In particular kernel_execution does NOT assume allowed or enabled.
pub open spec fn callback_order(s: System, es: Seq<Event>) -> bool
    decreases es.len(),
{
    es.len() == 0 || ((match es[0] {
        Event::Receipt { slot, op } => allowed(s.calls[slot].pc, op) && cas_sound(op),
        Event::End { slot } => s.calls[slot].pc == Pc::Start || s.calls[slot].pc == Pc::Old
            || s.calls[slot].pc == Pc::Empty || s.calls[slot].pc == Pc::Done,
        _ => true,
    }) && callback_order(advance_system(s, es[0]), es.subrange(1, es.len() as int)))
}
pub open spec fn kernel_execution(s: System, es: Seq<Event>) -> bool
    decreases es.len(),
{
    es.len() == 0 || ((match es[0] {
        Event::Receipt { slot, op } => active(s, slot) && primitive_coherent(s, slot, op),
        Event::End { slot } => active(s, slot),
        _ => permitted(s, es[0]),
    }) && kernel_execution(advance_system(s, es[0]), es.subrange(1, es.len() as int)))
}
pub proof fn compose_orders(s: System, es: Seq<Event>)
    requires callback_order(s, es), kernel_execution(s, es),
    ensures admissible(s, es),
    decreases es.len(),
{
    if es.len() > 0 {
        compose_orders(advance_system(s, es[0]), es.subrange(1, es.len() as int));
    }
}
pub open spec fn receipt_step(s: System, k: int, o: Op) -> Step {
    let t = s.calls[k].task;
    match o {
        Op::Cas { kind, before, after, observed } => if before != observed { Step::Stutter }
            else if kind == REGISTER { Step::Register { task: t, after } }
            else if kind == RELEASE { Step::Release { task: t, after } }
            else if kind == PUBLISH { Step::Publish { covered: c(after), after } }
            else { Step::Retire { after } },
        Op::TicketWrite { owned } => if owned { Step::Own { task: t } } else { Step::Clear { task: t } },
        Op::Insert { target: true } => Step::Request { task: t },
        _ => Step::Stutter,
    }
}
pub open spec fn lift(s: System, e: Event) -> Step {
    match e {
        Event::Receipt { slot, op } => receipt_step(s, slot, op),
        Event::KernelLand { task } => Step::Land { task },
        Event::KernelRemove { task } => Step::Remove { task },
        Event::KernelCancel { task } => Step::Cancel { task },
        _ => Step::Stutter,
    }
}
pub open spec fn advance_system(s: System, e: Event) -> System {
    let shared = next(s.shared, lift(s, e));
    let calls = match e {
        Event::Begin { slot, task } => s.calls.update(slot, Call { task, pc: Pc::Start, active: true }),
        Event::End { slot } => s.calls.update(slot, Call { active: false, ..s.calls[slot] }),
        Event::Receipt { slot, op } => s.calls.update(slot, Call {
            pc: advance(s.calls[slot].pc, op), ..s.calls[slot]
        }),
        _ => s.calls,
    };
    System { shared, calls }
}

// This is the missing obligation in the previous implementation: derive enabled
// from callback order + primitive facts, rather than require it as a premise.
pub proof fn receipt_enables_model(s: System, k: int, o: Op)
    requires system_inv(s), permitted(s, Event::Receipt { slot: k, op: o }),
    ensures enabled(s.shared, receipt_step(s, k, o)),
{
    assert(coupled(s, k));
    match o {
        Op::Cas { kind: REGISTER | RELEASE, .. } => {
            assert(s.calls[k].task >= 0);
        },
        _ => {},
    }
}

pub proof fn event_refines(s: System, e: Event)
    requires system_inv(s), permitted(s, e),
    ensures enabled(s.shared, lift(s, e)), system_inv(advance_system(s, e)),
        advance_system(s, e).shared == next(s.shared, lift(s, e)),
{
    match e {
        Event::Receipt { slot, op } => { receipt_enables_model(s, slot, op); },
        _ => {},
    }
    preserves(s.shared, lift(s, e));
    let r = advance_system(s, e);
    assert forall|k: int| 0 <= k < r.calls.len() implies coupled(r, k) by {
        assert(coupled(s, k));
        match e {
            Event::Receipt { slot, op } => {
                if slot != k && s.calls[k].active && s.calls[k].task >= 0 {
                    assert(s.calls[k].task != s.calls[slot].task);
                }
                match op {
                    Op::Cas { .. } => {},
                    _ => {},
                }
            },
            _ => {},
        }
    }
}

// Direct entry from a verified callback Log::wf() certificate. The global merge
// must preserve each callback's local prefix order, but may run other CPUs
// between any two receipts. Primitive facts do not assert model::enabled.
pub proof fn checked_receipt_refines(s: System, k: int, xs: Seq<Op>, end: Pc, i: nat)
    requires system_inv(s), active(s, k), follows(Pc::Start, xs, end), i < xs.len(),
        s.calls[k].pc == after_prefix(Pc::Start, xs, i),
        primitive_coherent(s, k, xs[i as int]),
    ensures enabled(s.shared, receipt_step(s, k, xs[i as int])),
        system_inv(advance_system(s, Event::Receipt { slot: k, op: xs[i as int] })),
{
    checked_prefix(Pc::Start, xs, end, i);
    event_refines(s, Event::Receipt { slot: k, op: xs[i as int] });
}

pub open spec fn admissible(s: System, es: Seq<Event>) -> bool
    decreases es.len(),
{
    es.len() == 0 || (permitted(s, es[0])
        && admissible(advance_system(s, es[0]), es.subrange(1, es.len() as int)))
}
pub open spec fn execute(s: System, es: Seq<Event>) -> System
    decreases es.len(),
{
    if es.len() == 0 { s } else { execute(advance_system(s, es[0]), es.subrange(1, es.len() as int)) }
}
pub proof fn callbacks_refine_protocol(s: System, es: Seq<Event>)
    requires system_inv(s), admissible(s, es),
    ensures system_inv(execute(s, es)),
        project(execute(s, es).shared).queued > 0 && !project(execute(s, es).shared).covered
            ==> project(execute(s, es).shared).drain,
    decreases es.len(),
{
    if es.len() > 0 {
        event_refines(s, es[0]);
        callbacks_refine_protocol(advance_system(s, es[0]), es.subrange(1, es.len() as int));
    } else { linked(s.shared); }
}

// callback_order comes from the checked Log/prefix theorem and return contracts;
// kernel_execution is the explicit primitive/lifecycle interpretation boundary.
pub proof fn checked_callbacks_are_safe(s: System, es: Seq<Event>)
    requires system_inv(s), callback_order(s, es), kernel_execution(s, es),
    ensures system_inv(execute(s, es)),
        project(execute(s, es).shared).queued > 0 && !project(execute(s, es).shared).covered
            ==> project(execute(s, es).shared).drain,
{
    compose_orders(s, es);
    callbacks_refine_protocol(s, es);
}

// Non-vacuity: config and drain both execute after registration but BEFORE the
// physical ticket store. Landing later still leaves the orphan queue drainable.
pub proof fn reserved_window_is_admitted()
{
    reveal_with_fuel(total, 2);
    reveal_with_fuel(qtotal, 2);
    assert(legal(5) && safe(5) && n(5) == 1) by(bit_vector);
    assert(!c(6) && d(6) && c(5) && 5u64 >> 2 != 0u64) by(bit_vector);
    assert(6u64 == (5u64 & !1u64) | 2u64 && 6u64 >> 2 != 0u64) by(bit_vector);
    let s = System {
        shared: State { word: 5, tasks: seq![Phase::Reserved] },
        calls: seq![Call { task: 0, pc: Pc::Reserved, active: true },
                    Call { task: -1, pc: Pc::Start, active: true }],
    };
    assert(system_inv(s));
    let publish = Event::Receipt { slot: 1, op: Op::Cas { kind: PUBLISH, before: 5, after: 6, observed: 5 } };
    event_refines(s, publish);
    let s1 = advance_system(s, publish);
    let retire = Event::Receipt { slot: 1, op: Op::Cas { kind: RETIRE, before: 6, after: 6, observed: 6 } };
    event_refines(s1, retire);
    let s2 = advance_system(s1, retire);
    let own = Event::Receipt { slot: 0, op: Op::TicketWrite { owned: true } };
    event_refines(s2, own);
    let s3 = advance_system(s2, own);
    let request = Event::Receipt { slot: 0, op: Op::Insert { target: true } };
    event_refines(s3, request);
    let s4 = advance_system(s3, request);
    let land = Event::KernelLand { task: 0 };
    event_refines(s4, land);
    let r = advance_system(s4, land);
    assert(qtotal(r.shared.tasks) == 1);
    assert(!c(r.shared.word) && d(r.shared.word));
}
}
