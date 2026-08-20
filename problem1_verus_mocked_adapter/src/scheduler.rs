#![allow(dead_code)]

use crate::types::{Dispatch, DispatchTarget, RunMode, Snapshot, TaskRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyInput {
    Enqueued {
        task: TaskRef,
        is_problem_workload: bool,
    },
    DispatchTick,
    RecoveryDeadlineElapsed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyAction {
    HoldGate,
    StableInvalidState,
    Dispatch(Dispatch),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyCore {
    mode: RunMode,
    target_llc_queue: Vec<TaskRef>,
    published_target_llc: bool,
    drain_target_llc: bool,
    mask_generation: u64,
    selected_once: bool,
    invalid_reported: bool,
    recovered: bool,
}

impl PolicyCore {
    pub fn new(mode: RunMode) -> Self {
        Self {
            mode,
            target_llc_queue: Vec::new(),
            published_target_llc: true,
            drain_target_llc: false,
            mask_generation: 0,
            selected_once: false,
            invalid_reported: false,
            recovered: false,
        }
    }

    pub fn step(&mut self, input: PolicyInput) -> Vec<PolicyAction> {
        let mut actions = Vec::new();
        match input {
            PolicyInput::Enqueued {
                task,
                is_problem_workload,
            } => self.on_enqueue(task, is_problem_workload, &mut actions),
            PolicyInput::DispatchTick => self.on_dispatch_tick(&mut actions),
            PolicyInput::RecoveryDeadlineElapsed => {
                self.on_recovery_deadline_elapsed(&mut actions)
            }
        }
        actions
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            mask_generation: self.mask_generation,
            q: self.target_llc_queue.len(),
            c: self.published_target_llc,
            d: self.drain_target_llc,
            selected_once: self.selected_once,
            recovered: self.recovered,
        }
    }

    pub fn scheduled_len(&self) -> usize {
        self.target_llc_queue.len()
    }

    pub fn recovered(&self) -> bool {
        self.recovered
    }

    pub fn selected_once(&self) -> bool {
        self.selected_once
    }

    fn on_enqueue(
        &mut self,
        task: TaskRef,
        is_problem_workload: bool,
        actions: &mut Vec<PolicyAction>,
    ) {
        if !is_problem_workload {
            self.push_dispatch(actions, task, DispatchTarget::Any);
            return;
        }

        if !self.selected_once {
            self.selected_once = true;

            if self.mode == RunMode::Deterministic {
                actions.push(PolicyAction::HoldGate);
            }

            self.publish_mask_without_target_llc();
            self.update_observe_queue();
            self.enqueue_commit_to_target_llc(task);
        } else if self.published_target_llc {
            self.enqueue_commit_to_target_llc(task);
        } else {
            self.push_dispatch(actions, task, DispatchTarget::RecoveryCpu);
        }
    }

    fn on_dispatch_tick(&mut self, actions: &mut Vec<PolicyAction>) {
        if self.target_llc_queue.is_empty() {
            return;
        }

        if self.published_target_llc {
            let task = self.pop_front_task();
            self.push_dispatch(actions, task, DispatchTarget::TargetCpu);
            return;
        }

        if self.drain_target_llc {
            let task = self.pop_front_task();
            self.recovered = true;
            self.push_dispatch(actions, task, DispatchTarget::RecoveryCpu);
            return;
        }

        if !self.invalid_reported {
            self.invalid_reported = true;
            actions.push(PolicyAction::StableInvalidState);
        }
    }

    fn on_recovery_deadline_elapsed(&mut self, _actions: &mut Vec<PolicyAction>) {
        if !self.target_llc_queue.is_empty()
            && !self.published_target_llc
            && !self.drain_target_llc
        {
            self.drain_target_llc = true;
        }
    }

    fn publish_mask_without_target_llc(&mut self) {
        self.published_target_llc = false;
        self.mask_generation += 1;
    }

    fn update_observe_queue(&mut self) {
        if !self.published_target_llc && !self.target_llc_queue.is_empty() {
            self.drain_target_llc = true;
        }
    }

    fn enqueue_commit_to_target_llc(&mut self, task: TaskRef) {
        self.target_llc_queue.push(task);
    }

    fn pop_front_task(&mut self) -> TaskRef {
        self.target_llc_queue.remove(0)
    }

    fn push_dispatch(
        &self,
        actions: &mut Vec<PolicyAction>,
        task: TaskRef,
        target: DispatchTarget,
    ) {
        actions.push(PolicyAction::Dispatch(Dispatch {
            task,
            target,
            snapshot: self.snapshot(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> TaskRef {
        TaskRef::new(100, 1)
    }

    #[test]
    fn non_problem_task_dispatches_any() {
        let mut policy = PolicyCore::new(RunMode::Report);

        let actions = policy.step(PolicyInput::Enqueued {
            task: task(),
            is_problem_workload: false,
        });

        assert!(matches!(
            actions.as_slice(),
            [PolicyAction::Dispatch(Dispatch {
                target: DispatchTarget::Any,
                ..
            })]
        ));
        assert_eq!(policy.scheduled_len(), 0);
    }

    #[test]
    fn first_deterministic_enqueue_reaches_stranded_state() {
        let mut policy = PolicyCore::new(RunMode::Deterministic);

        let actions = policy.step(PolicyInput::Enqueued {
            task: task(),
            is_problem_workload: true,
        });

        assert_eq!(actions, vec![PolicyAction::HoldGate]);
        assert_eq!(
            policy.snapshot(),
            Snapshot {
                mask_generation: 1,
                q: 1,
                c: false,
                d: false,
                selected_once: true,
                recovered: false,
            }
        );
    }

    #[test]
    fn recovery_deadline_enables_drain_then_dispatches_recovery() {
        let mut policy = PolicyCore::new(RunMode::Deterministic);
        let _ = policy.step(PolicyInput::Enqueued {
            task: task(),
            is_problem_workload: true,
        });

        let invalid = policy.step(PolicyInput::DispatchTick);
        assert_eq!(invalid, vec![PolicyAction::StableInvalidState]);

        let recovery = policy.step(PolicyInput::RecoveryDeadlineElapsed);
        assert!(recovery.is_empty());
        assert!(policy.snapshot().d);

        let dispatch = policy.step(PolicyInput::DispatchTick);
        assert!(matches!(
            dispatch.as_slice(),
            [PolicyAction::Dispatch(Dispatch {
                target: DispatchTarget::RecoveryCpu,
                ..
            })]
        ));
        assert!(policy.recovered());
        assert_eq!(policy.scheduled_len(), 0);
    }
}
