// SPDX-License-Identifier: GPL-2.0
// Fine-grained implementation state. No whole callback is atomic here.
use vstd::prelude::*;
use crate::word::*;
verus! {

#[derive(PartialEq, Eq)]
pub enum Phase { Off, Reserved, Owned, Requested, Queued, Detached, Releasing }

pub open spec fn tracked(p: Phase) -> int {
    if p == Phase::Off || p == Phase::Releasing { 0 } else { 1 }
}
pub open spec fn queued(p: Phase) -> int { if p == Phase::Queued { 1 } else { 0 } }
pub open spec fn total(ps: Seq<Phase>) -> int
    decreases ps.len(),
{
    if ps.len() == 0 { 0 } else { tracked(ps[0]) + total(ps.subrange(1, ps.len() as int)) }
}
pub open spec fn qtotal(ps: Seq<Phase>) -> int
    decreases ps.len(),
{
    if ps.len() == 0 { 0 } else { queued(ps[0]) + qtotal(ps.subrange(1, ps.len() as int)) }
}

pub struct State { pub word: u64, pub tasks: Seq<Phase> }
pub open spec fn inv(s: State) -> bool {
    legal(s.word) && safe(s.word) && n(s.word) == total(s.tasks)
}

// before/after are the actual successful CAS operands, not a guessed snapshot.
pub enum Step {
    Register { task: int, after: u64 },
    Own { task: int },
    Request { task: int },
    Land { task: int },
    Remove { task: int },
    Cancel { task: int },
    Release { task: int, after: u64 },
    Clear { task: int },
    Publish { covered: bool, after: u64 },
    Retire { after: u64 },
    Stutter,
}
pub open spec fn at(s: State, t: int, p: Phase) -> bool {
    0 <= t < s.tasks.len() && s.tasks[t] == p
}
pub open spec fn enabled(s: State, a: Step) -> bool {
    match a {
        Step::Register { task, after } => at(s, task, Phase::Off) && n(s.word) < 65535
            && after == ((s.word + 4) as u64 | if c(s.word) { 0u64 } else { 2u64 }),
        Step::Own { task } => at(s, task, Phase::Reserved),
        Step::Request { task } => at(s, task, Phase::Owned),
        Step::Land { task } => at(s, task, Phase::Requested),
        Step::Remove { task } => at(s, task, Phase::Queued),
        Step::Cancel { task } => at(s, task, Phase::Requested),
        Step::Release { task, after } => at(s, task, Phase::Detached)
            && after == (s.word - 4) as u64,
        Step::Clear { task } => at(s, task, Phase::Releasing),
        Step::Publish { covered, after } => after == ((s.word & !1u64)
            | if covered { 1u64 } else if s.word >> 2 != 0 { 2u64 } else { 0u64 }),
        Step::Retire { after } => after == if s.word >> 2 == 0 { s.word & !2u64 } else { s.word },
        Step::Stutter => true,
    }
}
pub open spec fn next(s: State, a: Step) -> State {
    match a {
        Step::Register { task, after } => State { word: after, tasks: s.tasks.update(task, Phase::Reserved) },
        Step::Own { task } => State { tasks: s.tasks.update(task, Phase::Owned), ..s },
        Step::Request { task } => State { tasks: s.tasks.update(task, Phase::Requested), ..s },
        Step::Land { task } => State { tasks: s.tasks.update(task, Phase::Queued), ..s },
        Step::Remove { task } | Step::Cancel { task } => State { tasks: s.tasks.update(task, Phase::Detached), ..s },
        Step::Release { task, after } => State { word: after, tasks: s.tasks.update(task, Phase::Releasing) },
        Step::Clear { task } => State { tasks: s.tasks.update(task, Phase::Off), ..s },
        Step::Publish { after, .. } | Step::Retire { after } => State { word: after, ..s },
        Step::Stutter => s,
    }
}

pub proof fn totals(ps: Seq<Phase>)
    ensures 0 <= qtotal(ps) <= total(ps),
    decreases ps.len(),
{
    if ps.len() > 0 { totals(ps.subrange(1, ps.len() as int)); }
}
pub proof fn replacement(ps: Seq<Phase>, i: int, p: Phase)
    requires 0 <= i < ps.len(),
    ensures total(ps.update(i, p)) == total(ps) - tracked(ps[i]) + tracked(p),
        qtotal(ps.update(i, p)) == qtotal(ps) - queued(ps[i]) + queued(p),
    decreases ps.len(),
{
    let tail = ps.subrange(1, ps.len() as int);
    if i == 0 {
        assert(ps.update(i, p).subrange(1, ps.len() as int) =~= tail);
    } else {
        replacement(tail, i - 1, p);
        assert(ps.update(i, p).subrange(1, ps.len() as int) =~= tail.update(i - 1, p));
    }
}

pub proof fn initial(ps: Seq<Phase>)
    requires forall|i: int| 0 <= i < ps.len() ==> ps[i] == Phase::Off,
    ensures inv(State { word: 1, tasks: ps }),
    decreases ps.len(),
{
    if ps.len() > 0 { initial(ps.subrange(1, ps.len() as int)); }
    assert(legal(1) && safe(1) && n(1) == 0) by(bit_vector);
}

pub proof fn preserves(s: State, a: Step)
    requires inv(s), enabled(s, a),
    ensures inv(next(s, a)),
{
    let w = s.word;
    match a {
        Step::Register { task, after } => {
            replacement(s.tasks, task, Phase::Reserved);
            assert(legal(after) && safe(after) && n(after) == n(w) + 1) by(bit_vector)
                requires legal(w), safe(w), n(w) < 65535,
                    after == ((w + 4) as u64 | if c(w) { 0u64 } else { 2u64 });
        },
        Step::Release { task, after } => {
            replacement(s.tasks, task, Phase::Releasing);
            totals(s.tasks.update(task, Phase::Releasing));
            assert(n(s.word) > 0);
            assert(legal(after) && safe(after) && n(after) + 1 == n(w)) by(bit_vector)
                requires legal(w), safe(w), n(w) > 0, after == (w - 4) as u64;
        },
        Step::Publish { covered, after } => {
            assert(legal(after) && safe(after) && n(after) == n(w)) by(bit_vector)
                requires legal(w), after == ((w & !1u64)
                    | if covered { 1u64 } else if w >> 2 != 0 { 2u64 } else { 0u64 });
        },
        Step::Retire { after } => {
            assert(legal(after) && safe(after) && n(after) == n(w)) by(bit_vector)
                requires legal(w), safe(w),
                    after == if w >> 2 == 0 { w & !2u64 } else { w };
        },
        Step::Own { task } => { replacement(s.tasks, task, Phase::Owned); },
        Step::Request { task } => { replacement(s.tasks, task, Phase::Requested); },
        Step::Land { task } => { replacement(s.tasks, task, Phase::Queued); },
        Step::Remove { task } | Step::Cancel { task } => { replacement(s.tasks, task, Phase::Detached); },
        Step::Clear { task } => { replacement(s.tasks, task, Phase::Off); },
        Step::Stutter => {},
    }
}
}
