// SPDX-License-Identifier: GPL-2.0
// Verified callback-local monitor. This is ghost-only, not another scheduler API.
use crate::*;
#[cfg(verus_keep_ghost)]
use crate::word::*;
verus! {
#[derive(PartialEq, Eq)]
pub enum Pc { Start, Old, Empty, Reserved, Owned, Releasing, Done }

pub open spec fn cas_sound(o: Op) -> bool {
    match o {
        Op::Cas { kind, before: w, after: r, .. } => legal(w) && match kind {
            REGISTER => n(w) < 65535 && r == ((w + 4) as u64 | if c(w) { 0u64 } else { 2u64 }),
            RELEASE => n(w) > 0 && r == (w - 4) as u64,
            PUBLISH => r == ((w & !1u64) | if c(r) { 1u64 } else if w >> 2 != 0 { 2u64 } else { 0u64 }),
            RETIRE => r == if w >> 2 == 0 { w & !2u64 } else { w },
            _ => false,
        },
        _ => true,
    }
}
pub open spec fn allowed(pc: Pc, o: Op) -> bool {
    match o {
        Op::TicketRead { .. } => pc == Pc::Start,
        Op::TicketWrite { owned } => if owned { pc == Pc::Reserved } else { pc == Pc::Releasing },
        Op::Cas { kind, .. } => match kind {
            REGISTER => pc == Pc::Empty,
            RELEASE => pc == Pc::Old,
            PUBLISH | RETIRE => pc == Pc::Start,
            _ => false,
        },
        Op::Insert { target } => if target { pc == Pc::Owned }
            else { pc == Pc::Start || pc == Pc::Empty || pc == Pc::Old },
        Op::Move { .. } => pc == Pc::Start,
    }
}
pub open spec fn advance(pc: Pc, o: Op) -> Pc {
    match o {
        Op::TicketRead { owned } => if owned { Pc::Old } else { Pc::Empty },
        Op::TicketWrite { owned } => if owned { Pc::Owned } else { Pc::Empty },
        Op::Cas { kind, before, observed, .. } => if before != observed { pc }
            else if kind == REGISTER { Pc::Reserved }
            else if kind == RELEASE { Pc::Releasing } else { pc },
        Op::Insert { .. } => Pc::Done,
        Op::Move { .. } => pc,
    }
}
pub open spec fn follows(pc: Pc, xs: Seq<Op>, end: Pc) -> bool
    decreases xs.len(),
{
    if xs.len() == 0 { pc == end } else {
        allowed(pc, xs[0]) && cas_sound(xs[0])
            && follows(advance(pc, xs[0]), xs.subrange(1, xs.len() as int), end)
    }
}
pub proof fn append(pc: Pc, xs: Seq<Op>, end: Pc, o: Op)
    requires follows(pc, xs, end), allowed(end, o), cas_sound(o),
    ensures follows(pc, xs.push(o), advance(end, o)),
    decreases xs.len(),
{
    reveal_with_fuel(follows, 2);
    if xs.len() > 0 {
        append(advance(pc, xs[0]), xs.subrange(1, xs.len() as int), end, o);
        assert(xs.push(o).subrange(1, xs.len() as int + 1) =~= xs.subrange(1, xs.len() as int).push(o));
    } else {
        assert(xs.push(o)[0] == o);
        assert(xs.push(o).subrange(1, 1) =~= Seq::<Op>::empty());
    }
}

// A finished (or paused) checked callback log certifies every prefix. Other CPUs
// may run between its API calls without invalidating its local order.
pub open spec fn after_prefix(pc: Pc, xs: Seq<Op>, k: nat) -> Pc
    recommends k <= xs.len(),
    decreases k,
{
    if k == 0 { pc } else {
        after_prefix(advance(pc, xs[0]), xs.subrange(1, xs.len() as int), (k - 1) as nat)
    }
}
pub proof fn checked_prefix(pc: Pc, xs: Seq<Op>, end: Pc, k: nat)
    requires follows(pc, xs, end), k < xs.len(),
    ensures allowed(after_prefix(pc, xs, k), xs[k as int]), cas_sound(xs[k as int]),
    decreases k,
{
    if k > 0 {
        checked_prefix(advance(pc, xs[0]), xs.subrange(1, xs.len() as int), end, (k - 1) as nat);
    }
}

}
