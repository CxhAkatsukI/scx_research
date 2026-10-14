// SPDX-License-Identifier: GPL-2.0
#![no_std]
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
pub use lachesis_runtime_trusted::{primitives::*, log::Log, task::Task, scheduler, config_entry};
pub use p1_protocol::{*, receipts::*, word::*};

verus! {
pub struct Shared { pub packed: AtomicWord }

pub open spec fn update_pc(k: u8, p: Pc) -> bool {
    (k == REGISTER && p == Pc::Empty) || (k == RELEASE && p == Pc::Old)
        || ((k == PUBLISH || k == RETIRE) && p == Pc::Start)
}
pub open spec fn success_pc(k: u8, p: Pc) -> Pc {
    if k == REGISTER { Pc::Reserved } else if k == RELEASE { Pc::Releasing } else { p }
}

impl Shared {
    // Each attempt can be interleaved with arbitrary other CPUs.
    pub fn update(&self, kind: u8, covered: bool, log: &mut Log) -> (ok: bool)
        requires old(log).wf(), update_pc(kind, old(log).position()),
        ensures final(log).wf(), final(log).position() == if ok {
            success_pc(kind, old(log).position())
        } else { old(log).position() },
    {
        let ghost start = log.position();
        let mut before = self.packed.snapshot();
        let mut tries: u32 = 0;
        while tries < 32
            invariant log.wf(), log.position() == start, start == old(log).position(),
                update_pc(kind, start), tries <= 32,
            decreases 32 - tries,
        {
            // Bounds are checked; legal/safe shared state is not assumed by this function.
            if before >= 262144 { return false; }
            let after = if kind == REGISTER {
                if count(before) == LIMIT { return false; }
                register(before)
            } else if kind == RELEASE {
                if count(before) == 0 { return false; }
                release(before)
            } else if kind == PUBLISH {
                publish(before, covered)
            } else { retire(before) };
            let observed = self.packed.cas(before, after, kind, log);
            if observed == before { return true; }
            before = observed;
            tries += 1;
        }
        false
    }

    // The entry context must establish that the old registration cannot land again.
    pub fn finish_removed(&self, t: &mut Ticket, log: &mut Log) -> (ok: bool)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), if ok { final(log).position() == Pc::Empty } else { final(log).position() == Pc::Old },
    {
        let owned = t.owned(log);
        if !owned { return true; }
        if !self.update(RELEASE, false, log) { return false; }
        t.set_owned(false, log);
        true
    }
}

// Callback contracts are about the checked monitor. The global composition theorem
// supplies shared semantics separately; it does not assume whole-callback atomicity.
pub trait Policy {
    fn select_cpu(&self, p: Task, prev: i32, flags: u64, log: &mut Log) -> (r: i32)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == Pc::Start, r == prev;
    fn enqueue(&self, p: Task, flags: u64, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == Pc::Done;
    fn dispatch(&self, cpu: i32, prev: Option<Task>, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == Pc::Start;
    fn running(&self, p: Task, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == Pc::Start || final(log).position() == Pc::Old || final(log).position() == Pc::Empty;
    fn disable(&self, p: Task, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == Pc::Start || final(log).position() == Pc::Old || final(log).position() == Pc::Empty;
    fn exit_task(&self, p: Task, args: u64, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == Pc::Start || final(log).position() == Pc::Old || final(log).position() == Pc::Empty;
    fn init(&self, log: &mut Log) -> (r: i32)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == Pc::Start, r <= 0;
}
}
