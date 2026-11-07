// SPDX-License-Identifier: GPL-2.0
// Callback-local receipts, following Lachesis. No shared-state safety is assumed.
use crate::*;
verus! {
#[derive(PartialEq, Eq)]
pub enum Pc { Enq, Counted, Enable, Kick, Scan, Clear, Recheck, Repair, Rekick, Done }
#[derive(PartialEq, Eq)]
pub enum Op { Inc, Covered(bool), Pending(u32), Set, Clear, Kick }

pub open spec fn allowed(p: Pc, o: Op, fixed: bool) -> bool {
    match o {
        Op::Inc => p == Pc::Enq,
        Op::Covered(_) => p == Pc::Counted,
        Op::Pending(_) => p == Pc::Scan || (p == Pc::Recheck && fixed),
        Op::Set => p == Pc::Enable || p == Pc::Repair,
        Op::Clear => p == Pc::Clear,
        Op::Kick => p == Pc::Kick || p == Pc::Rekick,
    }
}
pub open spec fn advance(p: Pc, o: Op, fixed: bool) -> Pc {
    match o {
        Op::Inc => Pc::Counted,
        Op::Covered(c) => if c { Pc::Done } else { Pc::Enable },
        Op::Pending(n) => if p == Pc::Scan {
            if n == 0 { Pc::Clear } else { Pc::Done }
        } else { if n == 0 { Pc::Done } else { Pc::Repair } },
        Op::Set => if p == Pc::Enable { Pc::Kick } else { Pc::Rekick },
        Op::Clear => if fixed { Pc::Recheck } else { Pc::Done },
        Op::Kick => Pc::Done,
    }
}
pub open spec fn follows(p: Pc, xs: Seq<Op>, end: Pc, fixed: bool) -> bool
    decreases xs.len(),
{
    xs.len() == 0 && p == end || xs.len() > 0 && allowed(p, xs[0], fixed)
        && follows(advance(p, xs[0], fixed), xs.subrange(1, xs.len() as int), end, fixed)
}
pub proof fn append(p: Pc, xs: Seq<Op>, end: Pc, o: Op, fixed: bool)
    requires follows(p, xs, end, fixed), allowed(end, o, fixed),
    ensures follows(p, xs.push(o), advance(end, o, fixed), fixed),
    decreases xs.len(),
{
    reveal_with_fuel(follows, 2);
    if xs.len() > 0 {
        append(advance(p, xs[0], fixed), xs.subrange(1, xs.len() as int), end, o, fixed);
        assert(xs.push(o).subrange(1, xs.len() as int + 1)
            =~= xs.subrange(1, xs.len() as int).push(o));
    } else {
        assert(xs.push(o).subrange(1, 1) =~= Seq::<Op>::empty());
    }
}
pub open spec fn prefix(p: Pc, xs: Seq<Op>, k: nat, fixed: bool) -> Pc
    recommends k <= xs.len(),
    decreases k,
{
    if k == 0 { p } else {
        prefix(advance(p, xs[0], fixed), xs.subrange(1, xs.len() as int), (k - 1) as nat, fixed)
    }
}
pub proof fn checked_prefix(p: Pc, xs: Seq<Op>, end: Pc, k: nat, fixed: bool)
    requires follows(p, xs, end, fixed), k < xs.len(),
    ensures allowed(prefix(p, xs, k, fixed), xs[k as int], fixed),
    decreases k,
{
    if k > 0 {
        checked_prefix(advance(p, xs[0], fixed), xs.subrange(1, xs.len() as int),
            end, (k - 1) as nat, fixed);
    }
}
pub proof fn prefix_next(p: Pc, xs: Seq<Op>, k: nat, fixed: bool)
    requires k < xs.len(),
    ensures prefix(p, xs, k + 1, fixed) == advance(prefix(p, xs, k, fixed), xs[k as int], fixed),
    decreases k,
{
    reveal_with_fuel(prefix, 2);
    if k > 0 {
        prefix_next(advance(p, xs[0], fixed), xs.subrange(1, xs.len() as int), (k - 1) as nat, fixed);
    }
}
pub proof fn prefix_end(p: Pc, xs: Seq<Op>, end: Pc, fixed: bool)
    requires follows(p, xs, end, fixed),
    ensures prefix(p, xs, xs.len(), fixed) == end,
    decreases xs.len(),
{
    if xs.len() > 0 {
        prefix_end(advance(p, xs[0], fixed), xs.subrange(1, xs.len() as int), end, fixed);
    }
}

#[cfg(verus_keep_ghost)]
pub struct Log { ops: Ghost<Seq<Op>>, start: Ghost<Pc>, pc: Ghost<Pc>, repaired: Ghost<bool> }
#[cfg(verus_keep_ghost)]
impl Log {
    pub closed spec fn history(&self) -> Seq<Op> { self.ops@ }
    pub closed spec fn position(&self) -> Pc { self.pc@ }
    pub closed spec fn origin(&self) -> Pc { self.start@ }
    pub closed spec fn fixed(&self) -> bool { self.repaired@ }
    pub open spec fn wf(&self) -> bool {
        follows(self.origin(), self.history(), self.position(), self.fixed())
    }
    pub fn new(enqueue: bool, fixed: bool) -> (r: Self)
        ensures r.wf(), r.position() == if enqueue { Pc::Enq } else { Pc::Scan },
            r.origin() == r.position(), r.fixed() == fixed, r.history() == Seq::<Op>::empty(),
    {
        let ghost p = if enqueue { Pc::Enq } else { Pc::Scan };
        Log { ops: Ghost(Seq::empty()), start: Ghost(p), pc: Ghost(p), repaired: Ghost(fixed) }
    }
    pub(crate) fn record(&mut self, o: Op)
        requires old(self).wf(), allowed(old(self).position(), o, old(self).fixed()),
        ensures final(self).wf(), final(self).fixed() == old(self).fixed(),
            final(self).origin() == old(self).origin(),
            final(self).position() == advance(old(self).position(), o, old(self).fixed()),
            final(self).history() == old(self).history().push(o),
    {
        proof {
            append(self.start@, self.ops@, self.pc@, o, self.repaired@);
            self.ops = Ghost(self.ops@.push(o));
            self.pc = Ghost(advance(self.pc@, o, self.repaired@));
        }
    }
}
}

#[cfg(not(verus_keep_ghost))]
pub struct Log;
#[cfg(not(verus_keep_ghost))]
impl Log {
    pub fn new(_enqueue: bool, _fixed: bool) -> Self { Self }
    pub(crate) fn record(&mut self, _o: Op) {}
}
