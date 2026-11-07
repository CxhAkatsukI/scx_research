// SPDX-License-Identifier: GPL-2.0
// Trusted machine bindings only. Shared coherence is a separate, explicit premise.
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::{*, monitor::*};

#[cfg(host_replay)]
unsafe extern "C" { fn case_before(kind: u8); fn case_after(kind: u8, value: u32); }
fn before(_kind: u8) { #[cfg(host_replay)] unsafe { case_before(_kind); } }
fn after(_kind: u8, _value: u32) { #[cfg(host_replay)] unsafe { case_after(_kind, _value); } }

verus! {
#[verifier::external_body]
pub struct Counter { inner: AtomicU32 }
#[verifier::external_body]
pub struct Coverage { inner: AtomicU64 }
#[verifier::external_body]
pub struct DrainMask { inner: AtomicU64 }

impl Counter {
    #[verifier::external_body]
    pub fn new(n: u32) -> Self { Self { inner: AtomicU32::new(n) } }
    #[verifier::external_body]
    pub fn snapshot(&self) -> u32 { self.inner.load(Ordering::Relaxed) }
    #[verifier::external_body]
    fn add_raw(&self) {
        before(0);
        self.inner.fetch_add(1, Ordering::SeqCst);
        after(0, 0);
    }
    #[verifier::external_body]
    fn load_raw(&self) -> u32 {
        before(2);
        let r = self.inner.load(Ordering::Relaxed);
        after(2, r);
        r
    }
    pub fn fetch_add(&self, log: &mut Log)
        requires old(log).wf(), allowed(old(log).position(), Op::Inc, old(log).fixed()),
        ensures final(log).wf(), final(log).fixed() == old(log).fixed(),
            final(log).origin() == old(log).origin(),
            final(log).history() == old(log).history().push(Op::Inc),
            final(log).position() == advance(old(log).position(), Op::Inc, old(log).fixed()),
    { self.add_raw(); log.record(Op::Inc); }
    pub fn load(&self, log: &mut Log) -> (r: u32)
        requires old(log).wf(), allowed(old(log).position(), Op::Pending(0), old(log).fixed()),
        ensures final(log).wf(), final(log).fixed() == old(log).fixed(),
            final(log).origin() == old(log).origin(),
            final(log).history() == old(log).history().push(Op::Pending(r)),
            final(log).position() == advance(old(log).position(), Op::Pending(r), old(log).fixed()),
    { let r = self.load_raw(); log.record(Op::Pending(r)); r }
}
impl Coverage {
    #[verifier::external_body]
    pub fn new(covered: bool) -> Self { Self { inner: AtomicU64::new(covered as u64) } }
    #[verifier::external_body]
    fn load_raw(&self) -> bool {
        before(1);
        let r = self.inner.load(Ordering::Relaxed) & 1 != 0;
        after(1, r as u32);
        r
    }
    pub fn load(&self, log: &mut Log) -> (r: bool)
        requires old(log).wf(), old(log).position() == Pc::Counted,
        ensures final(log).wf(), final(log).fixed() == old(log).fixed(),
            final(log).origin() == old(log).origin(),
            final(log).history() == old(log).history().push(Op::Covered(r)),
            final(log).position() == if r { Pc::Done } else { Pc::Enable },
    { let r = self.load_raw(); log.record(Op::Covered(r)); r }
}
impl DrainMask {
    #[verifier::external_body]
    pub fn new(enabled: bool) -> Self { Self { inner: AtomicU64::new(enabled as u64) } }
    #[verifier::external_body]
    fn set_raw(&self) {
        before(3);
        self.inner.fetch_or(1, Ordering::SeqCst);
        after(3, 0);
    }
    #[verifier::external_body]
    fn clear_raw(&self) {
        before(4);
        self.inner.fetch_and(!1, Ordering::SeqCst);
        after(4, 0);
    }
    pub fn fetch_or(&self, log: &mut Log)
        requires old(log).wf(), allowed(old(log).position(), Op::Set, old(log).fixed()),
        ensures final(log).wf(), final(log).fixed() == old(log).fixed(),
            final(log).origin() == old(log).origin(),
            final(log).history() == old(log).history().push(Op::Set),
            final(log).position() == advance(old(log).position(), Op::Set, old(log).fixed()),
    { self.set_raw(); log.record(Op::Set); }
    pub fn fetch_and(&self, log: &mut Log)
        requires old(log).wf(), old(log).position() == Pc::Clear,
        ensures final(log).wf(), final(log).fixed() == old(log).fixed(),
            final(log).origin() == old(log).origin(),
            final(log).history() == old(log).history().push(Op::Clear),
            final(log).position() == if old(log).fixed() { Pc::Recheck } else { Pc::Done },
    { self.clear_raw(); log.record(Op::Clear); }
    #[verifier::external_body]
    pub fn snapshot(&self) -> bool { self.inner.load(Ordering::Relaxed) & 1 != 0 }
}

// The host replay records an invocation, not successful CPU selection or progress.
#[verifier::external_body]
fn kick_raw() { before(5); after(5, 0); }
pub fn kick_subcell_idle_cpu(log: &mut Log)
    requires old(log).wf(), allowed(old(log).position(), Op::Kick, old(log).fixed()),
    ensures final(log).wf(), final(log).fixed() == old(log).fixed(),
        final(log).origin() == old(log).origin(),
        final(log).history() == old(log).history().push(Op::Kick),
        final(log).position() == Pc::Done,
{ kick_raw(); log.record(Op::Kick); }
}
