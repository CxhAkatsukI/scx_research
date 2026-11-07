// SPDX-License-Identifier: GPL-2.0
// The executable callbacks must generate this local operation language.
use crate::*;
verus! {
#[derive(PartialEq, Eq)]
pub enum Pc { Insert, Coverage, Enable, Kick, Count, Clear, ConsumeLast, Consume, Recheck, Repair, Done }
#[derive(PartialEq, Eq)]
pub enum Op { Insert, Covered(bool), Queued(u32), Enable, Disable, Consume(bool), Kick }

pub open spec fn allowed(p: Pc, o: Op) -> bool {
    match o {
        Op::Insert => p == Pc::Insert,
        Op::Covered(_) => p == Pc::Coverage,
        Op::Queued(_) => p == Pc::Count || p == Pc::Recheck,
        Op::Enable => p == Pc::Enable || p == Pc::Repair,
        Op::Disable => p == Pc::Clear,
        Op::Consume(_) => p == Pc::Consume || p == Pc::ConsumeLast,
        Op::Kick => p == Pc::Kick,
    }
}
pub open spec fn advance(p: Pc, o: Op) -> Pc {
    match o {
        Op::Insert => Pc::Coverage,
        Op::Covered(c) => if c { Pc::Done } else { Pc::Enable },
        Op::Queued(n) => if p == Pc::Count {
            if n <= 1 { Pc::Clear } else { Pc::Consume }
        } else { if n == 0 { Pc::Done } else { Pc::Repair } },
        Op::Enable => if p == Pc::Enable { Pc::Kick } else { Pc::Done },
        Op::Disable => Pc::ConsumeLast,
        Op::Consume(_) => if p == Pc::ConsumeLast { Pc::Recheck } else { Pc::Done },
        Op::Kick => Pc::Done,
    }
}
pub open spec fn follows(p: Pc, xs: Seq<Op>, end: Pc) -> bool
    decreases xs.len(),
{
    if xs.len() == 0 { p == end } else {
        allowed(p, xs[0]) && follows(advance(p, xs[0]), xs.subrange(1, xs.len() as int), end)
    }
}
pub proof fn append(p: Pc, xs: Seq<Op>, end: Pc, o: Op)
    requires follows(p, xs, end), allowed(end, o),
    ensures follows(p, xs.push(o), advance(end, o)),
    decreases xs.len(),
{
    reveal_with_fuel(follows, 2);
    if xs.len() > 0 {
        append(advance(p, xs[0]), xs.subrange(1, xs.len() as int), end, o);
        assert(xs.push(o).subrange(1, xs.len() as int + 1)
            =~= xs.subrange(1, xs.len() as int).push(o));
    } else { assert(xs.push(o).subrange(1, 1) =~= Seq::<Op>::empty()); }
}
pub open spec fn prefix(p: Pc, xs: Seq<Op>, k: nat) -> Pc
    recommends k <= xs.len(),
    decreases k,
{
    if k == 0 { p } else {
        prefix(advance(p, xs[0]), xs.subrange(1, xs.len() as int), (k - 1) as nat)
    }
}
pub proof fn prefix_step(p: Pc, xs: Seq<Op>, end: Pc, k: nat)
    requires follows(p, xs, end), k < xs.len(),
    ensures allowed(prefix(p, xs, k), xs[k as int]),
        prefix(p, xs, k + 1) == advance(prefix(p, xs, k), xs[k as int]),
    decreases k,
{
    reveal_with_fuel(prefix, 2);
    if k > 0 { prefix_step(advance(p, xs[0]), xs.subrange(1, xs.len() as int), end, (k - 1) as nat); }
}
pub proof fn prefix_end(p: Pc, xs: Seq<Op>, end: Pc)
    requires follows(p, xs, end),
    ensures prefix(p, xs, xs.len()) == end,
    decreases xs.len(),
{
    if xs.len() > 0 { prefix_end(advance(p, xs[0]), xs.subrange(1, xs.len() as int), end); }
}
#[cfg(verus_keep_ghost)]
pub struct Log { ops: Ghost<Seq<Op>>, start: Ghost<Pc>, pc: Ghost<Pc> }
#[cfg(verus_keep_ghost)]
impl Log {
    pub closed spec fn history(&self) -> Seq<Op> { self.ops@ }
    pub closed spec fn position(&self) -> Pc { self.pc@ }
    pub closed spec fn origin(&self) -> Pc { self.start@ }
    pub open spec fn wf(&self) -> bool { follows(self.origin(), self.history(), self.position()) }
    pub fn new(enqueue: bool) -> (r: Self)
        ensures r.wf(), r.position() == if enqueue { Pc::Insert } else { Pc::Count },
            r.origin() == r.position(), r.history() == Seq::<Op>::empty(),
    {
        let ghost p = if enqueue { Pc::Insert } else { Pc::Count };
        Log { ops: Ghost(Seq::empty()), start: Ghost(p), pc: Ghost(p) }
    }
    pub(crate) fn record(&mut self, o: Op)
        requires old(self).wf(), allowed(old(self).position(), o),
        ensures final(self).wf(), final(self).origin() == old(self).origin(),
            final(self).position() == advance(old(self).position(), o),
            final(self).history() == old(self).history().push(o),
    {
        proof {
            append(self.start@, self.ops@, self.pc@, o);
            self.ops = Ghost(self.ops@.push(o));
            self.pc = Ghost(advance(self.pc@, o));
        }
    }
}
}
#[cfg(not(verus_keep_ghost))]
pub struct Log;
#[cfg(not(verus_keep_ghost))]
impl Log {
    pub fn new(_enqueue: bool) -> Self { Self }
    pub(crate) fn record(&mut self, _o: Op) {}
}
