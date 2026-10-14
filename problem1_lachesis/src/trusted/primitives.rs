// SPDX-License-Identifier: GPL-2.0
// Primitive bindings only. None assumes the drain invariant or a legal protocol step.
use core::sync::atomic::{AtomicU64 as CoreAtomic, Ordering};
use crate::{kfunc, log::Log, task::{Task, task_struct}, vprelude::*};
use p1_protocol::*;
#[cfg(verus_keep_ghost)]
use p1_protocol::receipts::*;

#[repr(C)]
pub struct TicketValue { owned: u64 }
#[repr(C)]
pub struct StorageMap {
    r#type: *const [u32; 29], // BPF_MAP_TYPE_TASK_STORAGE
    map_flags: *const [u32; 1], // BPF_F_NO_PREALLOC
    key: *const i32,
    value: *const TicketValue,
}
unsafe impl Sync for StorageMap {}
#[no_mangle]
#[link_section = ".maps"]
pub static tickets: StorageMap = StorageMap {
    r#type: core::ptr::null(), map_flags: core::ptr::null(),
    key: core::ptr::null(), value: core::ptr::null(),
};

verus! {
#[verifier::external_body]
pub struct AtomicWord { inner: CoreAtomic }
impl AtomicWord {
    #[verifier::external_body]
    pub const fn new(w: u64) -> Self { Self { inner: CoreAtomic::new(w) } }
    #[verifier::external_body]
    pub fn snapshot(&self) -> u64 {
        self.cas_raw(0, 0)
    }
    // Successful CAS is the linearization point; failure changes no local state.
    #[verifier::external_body]
    fn cas_raw(&self, before: u64, after: u64) -> (observed: u64) {
        match self.inner.compare_exchange(before, after, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(w) => w, Err(w) => w,
        }
    }
    pub fn cas(&self, before: u64, after: u64, kind: u8, log: &mut Log) -> (observed: u64)
        requires old(log).wf(), allowed(old(log).position(), Op::Cas { kind, before, after, observed: 0 }),
            cas_sound(Op::Cas { kind, before, after, observed: 0 }),
        ensures final(log).wf(),
            final(log).history() == old(log).history().push(Op::Cas { kind, before, after, observed }),
            final(log).position() == advance(old(log).position(), Op::Cas { kind, before, after, observed }),
    {
        let observed = self.cas_raw(before, after);
        log.record(Op::Cas { kind, before, after, observed });
        observed
    }
}
#[verifier::external_body]
pub struct Ticket { ptr: *mut TicketValue }
impl Ticket {
    #[verifier::external_body]
    fn read_raw(&self) -> bool { unsafe { core::ptr::read_volatile(&(*self.ptr).owned) != 0 } }
    #[verifier::external_body]
    fn write_raw(&mut self, owned: bool) {
        unsafe { core::ptr::write_volatile(&mut (*self.ptr).owned, owned as u64); }
    }
    pub fn owned(&self, log: &mut Log) -> (r: bool)
        requires old(log).wf(), old(log).position() == Pc::Start,
        ensures final(log).wf(), final(log).position() == if r { Pc::Old } else { Pc::Empty },
            final(log).history() == old(log).history().push(Op::TicketRead { owned: r }),
    {
        let r = self.read_raw();
        log.record(Op::TicketRead { owned: r });
        r
    }
    pub fn set_owned(&mut self, owned: bool, log: &mut Log)
        requires old(log).wf(), allowed(old(log).position(), Op::TicketWrite { owned }),
        ensures final(log).wf(), final(log).position() == if owned { Pc::Owned } else { Pc::Empty },
            final(log).history() == old(log).history().push(Op::TicketWrite { owned }),
    {
        self.write_raw(owned);
        log.record(Op::TicketWrite { owned });
    }
}
#[verifier::external_body]
pub fn ticket(p: &Task, create: bool) -> Option<Ticket> {
    type Get = unsafe extern "C" fn(*const StorageMap, *mut task_struct, *const TicketValue, u64) -> *mut TicketValue;
    let get: Get = unsafe { core::mem::transmute(156usize) };
    let ptr = unsafe { get(&tickets, p.as_ptr(), core::ptr::null(), create as u64) };
    if ptr.is_null() { None } else { Some(Ticket { ptr }) }
}
#[verifier::external_body]
fn insert_raw(p: &Task, dsq: u64, flags: u64) {
    unsafe { kfunc::scx_bpf_dsq_insert(p.as_ptr(), dsq, 20_000_000, flags); }
}
#[verifier::external_body]
fn move_raw(dsq: u64) -> bool { unsafe { kfunc::scx_bpf_dsq_move_to_local(dsq) } }
pub fn insert(p: &Task, dsq: u64, flags: u64, log: &mut Log)
    requires old(log).wf(), dsq == TARGET || dsq == GLOBAL,
        allowed(old(log).position(), Op::Insert { target: dsq == TARGET }),
    ensures final(log).wf(), final(log).position() == Pc::Done,
        final(log).history() == old(log).history().push(Op::Insert { target: dsq == TARGET }),
{
    insert_raw(p, dsq, flags);
    log.record(Op::Insert { target: dsq == TARGET });
}
pub fn move_to_local(dsq: u64, log: &mut Log) -> (moved: bool)
    requires old(log).wf(), old(log).position() == Pc::Start, dsq == TARGET,
    ensures final(log).wf(), final(log).position() == Pc::Start,
        final(log).history() == old(log).history().push(Op::Move { moved }),
{
    let moved = move_raw(dsq);
    log.record(Op::Move { moved });
    moved
}
#[verifier::external_body]
pub fn create_dsq(dsq: u64) -> (r: i32)
    ensures r <= 0,
{ unsafe { kfunc::scx_bpf_create_dsq(dsq, -1) } }
}
