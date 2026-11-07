// SPDX-License-Identifier: GPL-2.0
// Host bindings, not a BPF implementation. Shared semantics are explicit in model.rs.
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use crate::{*, monitor::*};
#[cfg(host_replay)]
unsafe extern "C" { fn layered_before(kind: u8); fn layered_after(kind: u8, value: u32); }
fn before(_kind: u8) { #[cfg(host_replay)] unsafe { layered_before(_kind); } }
fn after(_kind: u8, _v: u32) { #[cfg(host_replay)] unsafe { layered_after(_kind, _v); } }

verus! {
pub open spec fn operation(kind: u8, v: u32) -> Op {
    match kind {
        0 => Op::Insert, 1 => Op::Covered(v != 0), 2 => Op::Queued(v),
        3 => Op::Enable, 4 => Op::Disable, 5 => Op::Consume(v != 0), _ => Op::Kick,
    }
}
#[verifier::external_body]
pub struct Api { q: AtomicU32, d: AtomicBool, deferred: bool }
impl Api {
    #[verifier::external_body]
    pub fn new(deferred: bool) -> Self {
        Self { q: AtomicU32::new(1), d: AtomicBool::new(true), deferred }
    }
    #[verifier::external_body]
    fn raw(&self, kind: u8) -> u32 {
        before(kind);
        let v = match kind {
            0 => { if !self.deferred { self.q.fetch_add(1, Ordering::SeqCst); } 0 },
            1 => 0, // This slice starts after coverage was removed and stays uncovered.
            2 => self.q.load(Ordering::SeqCst),
            3 => { self.d.store(true, Ordering::SeqCst); 0 },
            4 => { self.d.store(false, Ordering::SeqCst); 0 },
            5 => {
                let q = self.q.load(Ordering::SeqCst);
                if q > 0 { self.q.fetch_sub(1, Ordering::SeqCst); 1 } else { 0 }
            },
            _ => 0,
        };
        after(kind, v);
        v
    }
    // This body is checked; only the underlying machine/API operation is trusted.
    fn call(&self, kind: u8, log: &mut Log) -> (v: u32)
        requires kind <= 6, old(log).wf(), allowed(old(log).position(), operation(kind, 0)),
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).history() == old(log).history().push(operation(kind, v)),
            final(log).position() == advance(old(log).position(), operation(kind, v)),
    {
        let v = self.raw(kind);
        let o = match kind {
            0 => Op::Insert, 1 => Op::Covered(v != 0), 2 => Op::Queued(v),
            3 => Op::Enable, 4 => Op::Disable, 5 => Op::Consume(v != 0), _ => Op::Kick,
        };
        log.record(o);
        v
    }
    pub fn dsq_insert(&self, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Insert,
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).history() == old(log).history().push(Op::Insert),
            final(log).position() == Pc::Coverage,
    { self.call(0, log); }
    pub fn nr_llc_cpus_nonzero(&self, log: &mut Log) -> (r: bool)
        requires old(log).wf(), old(log).position() == Pc::Coverage,
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).position() == if r { Pc::Done } else { Pc::Enable },
    { self.call(1, log) != 0 }
    pub fn dsq_nr_queued(&self, log: &mut Log) -> (r: u32)
        requires old(log).wf(), old(log).position() == Pc::Count || old(log).position() == Pc::Recheck,
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).position() == advance(old(log).position(), Op::Queued(r)),
    { self.call(2, log) }
    pub fn drain_enable(&self, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Enable || old(log).position() == Pc::Repair,
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).position() == advance(old(log).position(), Op::Enable),
    { self.call(3, log); }
    pub fn drain_disable(&self, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Clear,
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).position() == Pc::ConsumeLast,
    { self.call(4, log); }
    pub fn dsq_move_to_local(&self, log: &mut Log) -> (r: bool)
        requires old(log).wf(), old(log).position() == Pc::Consume || old(log).position() == Pc::ConsumeLast,
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).position() == advance(old(log).position(), Op::Consume(r)),
    { self.call(5, log) != 0 }
    pub fn kick_idle_node_cpu(&self, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Kick,
        ensures final(log).wf(), final(log).origin() == old(log).origin(),
            final(log).position() == Pc::Done,
    { self.call(6, log); }
    #[verifier::external_body]
    pub fn finish_enqueue(&self) {
        before(7); after(7, 0); // Callback return is not DSQ insertion.
        before(8);
        if self.deferred { self.q.fetch_add(1, Ordering::SeqCst); }
        after(8, 0);
    }
    #[verifier::external_body]
    pub fn snapshot(&self) -> (u32, bool) {
        (self.q.load(Ordering::SeqCst), self.d.load(Ordering::SeqCst))
    }
}
}
