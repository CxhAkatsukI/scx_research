#![no_std]

use core::ffi::c_void;
use sched_ext_verus::{scheduler_dispatch, scheduler_enqueue, scheduler_init, ScxApi};

extern "C" {
    fn scx_api_create_dsq(dsq_id: u64, node_id: i32) -> i32;
    fn scx_api_dsq_insert(
        task: *mut c_void,
        dsq_id: u64,
        slice_ns: u64,
        enq_flags: u64,
    );
    fn scx_api_dsq_move_to_local(dsq_id: u64) -> bool;
}

struct KernelApi;

impl ScxApi for KernelApi {
    type Task = *mut c_void;

    #[inline(always)]
    fn create_dsq(&mut self, dsq_id: u64, node_id: i32) -> i32 {
        unsafe { scx_api_create_dsq(dsq_id, node_id) }
    }

    #[inline(always)]
    fn dsq_insert(
        &mut self,
        task: *mut c_void,
        dsq_id: u64,
        slice_ns: u64,
        enq_flags: u64,
    ) {
        unsafe { scx_api_dsq_insert(task, dsq_id, slice_ns, enq_flags) }
    }

    #[inline(always)]
    fn dsq_move_to_local(&mut self, dsq_id: u64) -> bool {
        unsafe { scx_api_dsq_move_to_local(dsq_id) }
    }
}

#[no_mangle]
pub extern "C" fn rust_scheduler_init() -> i32 {
    scheduler_init(&mut KernelApi)
}

#[no_mangle]
pub extern "C" fn rust_scheduler_enqueue(task: *mut c_void, enq_flags: u64) {
    scheduler_enqueue(&mut KernelApi, task, enq_flags);
}

#[no_mangle]
pub extern "C" fn rust_scheduler_dispatch() {
    let _ = scheduler_dispatch(&mut KernelApi);
}
