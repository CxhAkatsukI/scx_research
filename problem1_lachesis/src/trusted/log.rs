// SPDX-License-Identifier: GPL-2.0
// Checked, ghost-only monitor. Only primitive wrappers can append receipts.
use crate::vprelude::*;
use p1_protocol::Op;
#[cfg(verus_keep_ghost)]
use p1_protocol::receipts::*;
pub const CB_SELECT_CPU: u8 = 0;
pub const CB_ENQUEUE: u8 = 1;
pub const CB_DEQUEUE: u8 = 2;
pub const CB_DISPATCH: u8 = 3;
pub const CB_RUNNING: u8 = 4;
pub const CB_STOPPING: u8 = 5;
pub const CB_ENABLE: u8 = 6;
pub const CB_UPDATE_IDLE: u8 = 7;
pub const CB_EXIT: u8 = 8;
pub const CB_INIT: u8 = 9;

verus! {
#[cfg(verus_keep_ghost)]
pub struct Log { ops: Ghost<Seq<Op>>, pc: Ghost<Pc> }
#[cfg(verus_keep_ghost)]
impl Log {
    pub closed spec fn history(&self) -> Seq<Op> { self.ops@ }
    pub closed spec fn position(&self) -> Pc { self.pc@ }
    pub open spec fn wf(&self) -> bool { follows(Pc::Start, self.history(), self.position()) }
    pub proof fn prefix_allowed(&self, k: nat)
        requires self.wf(), k < self.history().len(),
        ensures allowed(after_prefix(Pc::Start, self.history(), k), self.history()[k as int]),
            cas_sound(self.history()[k as int]),
    { checked_prefix(Pc::Start, self.history(), self.position(), k); }
    pub fn new(_cb: u8, _pid: i32) -> (r: Self)
        ensures r.wf(), r.position() == Pc::Start, r.history() == Seq::<Op>::empty(),
    { Log { ops: Ghost(Seq::empty()), pc: Ghost(Pc::Start) } }
    pub fn end(&mut self) {}
    pub(crate) fn record(&mut self, o: Op)
        requires old(self).wf(), allowed(old(self).position(), o), cas_sound(o),
        ensures final(self).wf(), final(self).history() == old(self).history().push(o),
            final(self).position() == advance(old(self).position(), o),
    {
        proof {
            append(Pc::Start, self.ops@, self.pc@, o);
            self.ops = Ghost(self.ops@.push(o));
            self.pc = Ghost(advance(self.pc@, o));
        }
    }
}
}

#[cfg(not(verus_keep_ghost))]
pub struct Log {}
#[cfg(not(verus_keep_ghost))]
impl Log {
    #[inline(always)]
    pub fn new(_cb: u8, _pid: i32) -> Self { Self {} }
    #[inline(always)]
    pub(crate) fn record(&mut self, _o: Op) {}
    #[inline(always)]
    pub fn end(&mut self) {}
}
