#![allow(dead_code)]

use crate::types::{Dispatch, TaskRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedTask {
    pub task: TaskRef,
    pub is_problem_workload: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MockAdapter {
    pub queued: Vec<QueuedTask>,
    pub dispatched: Vec<Dispatch>,
    pub slept_ms: u64,
    pub completed_pending: u64,
}

impl MockAdapter {
    pub fn new(queued: Vec<QueuedTask>) -> Self {
        Self {
            queued,
            dispatched: Vec::new(),
            slept_ms: 0,
            completed_pending: 0,
        }
    }

    pub fn dequeue(&mut self) -> Option<QueuedTask> {
        if self.queued.is_empty() {
            None
        } else {
            Some(self.queued.remove(0))
        }
    }

    pub fn dispatch(&mut self, dispatch: Dispatch) {
        self.dispatched.push(dispatch);
    }

    pub fn sleep_ms(&mut self, ms: u64) {
        self.slept_ms += ms;
    }

    pub fn notify_complete(&mut self, nr_pending: u64) {
        self.completed_pending = nr_pending;
    }
}
