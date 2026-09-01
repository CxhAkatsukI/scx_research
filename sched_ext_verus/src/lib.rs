#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(target_arch = "bpf")]
use verus_builtin_macros::verus;
#[cfg(not(target_arch = "bpf"))]
use vstd::prelude::*;

verus! {

pub const MAIN_DSQ: u64 = 0;
pub const NO_NUMA_NODE: i32 = -1;
pub const SLICE_NS: u64 = 5_000_000;

/// Kernel-shaped operations used by the scheduler policy.
pub trait ScxApi {
    type Task;

    spec fn dsq_exists(&self, dsq_id: u64) -> bool;
    spec fn queued_len(&self, dsq_id: u64) -> nat;

    fn create_dsq(&mut self, dsq_id: u64, node_id: i32) -> (result: i32)
        ensures
            result == 0 ==> final(self).dsq_exists(dsq_id),
            final(self).queued_len(dsq_id) == old(self).queued_len(dsq_id),
    ;

    fn dsq_insert(
        &mut self,
        task: Self::Task,
        dsq_id: u64,
        slice_ns: u64,
        enq_flags: u64,
    )
        requires
            old(self).dsq_exists(dsq_id),
        ensures
            final(self).dsq_exists(dsq_id),
            final(self).queued_len(dsq_id) == old(self).queued_len(dsq_id) + 1,
    ;

    fn dsq_move_to_local(&mut self, dsq_id: u64) -> (moved: bool)
        requires
            old(self).dsq_exists(dsq_id),
        ensures
            final(self).dsq_exists(dsq_id),
            moved <==> old(self).queued_len(dsq_id) > 0,
            moved ==> final(self).queued_len(dsq_id) + 1 == old(self).queued_len(dsq_id),
            !moved ==> final(self).queued_len(dsq_id) == old(self).queued_len(dsq_id),
    ;
}

/// Creates the scheduler's global queue.
pub fn scheduler_init<A: ScxApi>(api: &mut A) -> (result: i32)
    ensures
        result == 0 ==> final(api).dsq_exists(MAIN_DSQ),
        final(api).queued_len(MAIN_DSQ) == old(api).queued_len(MAIN_DSQ),
{
    api.create_dsq(MAIN_DSQ, NO_NUMA_NODE)
}

/// Places each runnable task on the single global FIFO queue.
pub fn scheduler_enqueue<A: ScxApi>(api: &mut A, task: A::Task, enq_flags: u64)
    requires
        old(api).dsq_exists(MAIN_DSQ),
    ensures
        final(api).dsq_exists(MAIN_DSQ),
        final(api).queued_len(MAIN_DSQ) == old(api).queued_len(MAIN_DSQ) + 1,
{
    api.dsq_insert(task, MAIN_DSQ, SLICE_NS, enq_flags);
}

/// Lets the current CPU consume one task if the global queue is non-empty.
pub fn scheduler_dispatch<A: ScxApi>(api: &mut A) -> (moved: bool)
    requires
        old(api).dsq_exists(MAIN_DSQ),
    ensures
        moved <==> old(api).queued_len(MAIN_DSQ) > 0,
        moved ==> final(api).queued_len(MAIN_DSQ) + 1 == old(api).queued_len(MAIN_DSQ),
        !moved ==> final(api).queued_len(MAIN_DSQ) == old(api).queued_len(MAIN_DSQ),
{
    api.dsq_move_to_local(MAIN_DSQ)
}

#[cfg(feature = "mock")]
pub mod mock {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MockTask {
        pub id: u64,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Event {
        CreateDsq { dsq_id: u64, node_id: i32 },
        Insert {
            task: MockTask,
            dsq_id: u64,
            slice_ns: u64,
            enq_flags: u64,
        },
        MoveToLocal { dsq_id: u64, moved: bool },
    }

    #[derive(Debug, Default)]
    pub struct MockApi {
        created: bool,
        queue: Vec<MockTask>,
        moved: Vec<MockTask>,
        events: Vec<Event>,
    }

    impl MockApi {
        pub fn queue(&self) -> &Vec<MockTask> {
            &self.queue
        }

        pub fn moved(&self) -> &Vec<MockTask> {
            &self.moved
        }

        pub fn events(&self) -> &Vec<Event> {
            &self.events
        }
    }

    impl ScxApi for MockApi {
        type Task = MockTask;

        closed spec fn dsq_exists(&self, dsq_id: u64) -> bool {
            dsq_id == MAIN_DSQ && self.created
        }

        closed spec fn queued_len(&self, dsq_id: u64) -> nat {
            if dsq_id == MAIN_DSQ { self.queue@.len() } else { 0 }
        }

        fn create_dsq(&mut self, dsq_id: u64, node_id: i32) -> (result: i32)
            ensures
                result == 0 <==> dsq_id == MAIN_DSQ,
                final(self).queued_len(MAIN_DSQ) == old(self).queued_len(MAIN_DSQ),
        {
            self.events.push(Event::CreateDsq { dsq_id, node_id });
            if dsq_id == MAIN_DSQ {
                self.created = true;
                0
            } else {
                -22
            }
        }

        fn dsq_insert(
            &mut self,
            task: MockTask,
            dsq_id: u64,
            slice_ns: u64,
            enq_flags: u64,
        ) {
            self.queue.push(task);
            self.events.push(Event::Insert {
                task,
                dsq_id,
                slice_ns,
                enq_flags,
            });
        }

        fn dsq_move_to_local(&mut self, dsq_id: u64) -> (moved: bool) {
            let moved = if self.queue.is_empty() {
                false
            } else {
                let task = self.queue.remove(0);
                self.moved.push(task);
                true
            };
            self.events.push(Event::MoveToLocal { dsq_id, moved });
            moved
        }
    }

    /// End-to-end executable proof for one task crossing the mock DSQ.
    pub fn verified_single_task_round_trip(api: &mut MockApi, task: MockTask) -> (result: i32)
        requires
            old(api).queued_len(MAIN_DSQ) == 0,
        ensures
            result == 0 ==> final(api).queued_len(MAIN_DSQ) == 0,
    {
        let result = scheduler_init(api);
        if result != 0 {
            return result;
        }

        scheduler_enqueue(api, task, 0);
        if !scheduler_dispatch(api) {
            return -1;
        }
        0
    }
}

} // verus!
