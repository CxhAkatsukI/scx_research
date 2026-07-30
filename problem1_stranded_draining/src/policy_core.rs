use crate::protocol::{CpuId, LlcId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyMode {
    Report,
    Deterministic,
    Stochastic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPlan {
    pub target_cpu: CpuId,
    pub target_llc: LlcId,
    pub recovery_cpu: CpuId,
    pub recovery_llc: LlcId,
    pub control_cpu: CpuId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TaskRef {
    pub pid: i32,
    pub enqueue_seq: u64,
}

impl TaskRef {
    pub fn new(pid: i32, enqueue_seq: u64) -> Self {
        Self { pid, enqueue_seq }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicySnapshot {
    pub mask_generation: u64,
    pub q: usize,
    pub c: bool,
    pub d: bool,
    pub selected_once: bool,
    pub recovered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyEventKind {
    WorkloadMatched,
    EnqueueSelect,
    PublishMask,
    UpdateObserveQueue,
    EnqueueCommit,
    StableInvalidState,
    RecoveryDrainEnabled,
}

impl PolicyEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkloadMatched => "workload_matched",
            Self::EnqueueSelect => "enqueue_select",
            Self::PublishMask => "publish_mask",
            Self::UpdateObserveQueue => "update_observe_queue",
            Self::EnqueueCommit => "enqueue_commit",
            Self::StableInvalidState => "stable_invalid_state",
            Self::RecoveryDrainEnabled => "recovery_drain_enabled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyEvent {
    pub kind: PolicyEventKind,
    pub task: Option<TaskRef>,
    pub cpu: Option<CpuId>,
    pub selected_target_llc: Option<LlcId>,
    pub snapshot: PolicySnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchTarget {
    Any,
    TargetCpu,
    RecoveryCpu,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchAction {
    pub task: TaskRef,
    pub target: DispatchTarget,
    pub snapshot: PolicySnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyAction {
    Emit(PolicyEvent),
    HoldGate,
    Dispatch(DispatchAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyInput {
    Enqueued {
        task: TaskRef,
        is_problem_workload: bool,
    },
    DispatchTick,
    RecoveryDeadlineElapsed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyCore {
    plan: PolicyPlan,
    mode: PolicyMode,
    target_q: Vec<TaskRef>,
    published_target_llc: bool,
    drain_target_llc: bool,
    mask_generation: u64,
    selected_once: bool,
    invalid_reported: bool,
    recovered: bool,
}

impl PolicyCore {
    pub fn new(plan: PolicyPlan, mode: PolicyMode) -> Self {
        Self {
            plan,
            mode,
            target_q: Vec::new(),
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
            PolicyInput::RecoveryDeadlineElapsed => self.on_recovery_deadline_elapsed(&mut actions),
        }
        actions
    }

    pub fn snapshot(&self) -> PolicySnapshot {
        PolicySnapshot {
            mask_generation: self.mask_generation,
            q: self.target_q.len(),
            c: self.published_target_llc,
            d: self.drain_target_llc,
            selected_once: self.selected_once,
            recovered: self.recovered,
        }
    }

    pub fn scheduled_len(&self) -> usize {
        self.target_q.len()
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
            self.push_event(
                actions,
                PolicyEventKind::WorkloadMatched,
                Some(task),
                Some(self.plan.target_cpu),
                Some(self.plan.target_llc),
            );
            self.push_event(
                actions,
                PolicyEventKind::EnqueueSelect,
                Some(task),
                Some(self.plan.target_cpu),
                Some(self.plan.target_llc),
            );

            if self.mode == PolicyMode::Deterministic {
                actions.push(PolicyAction::HoldGate);
            }

            self.publish_mask_without_target_llc(actions);
            self.update_observe_queue(actions);
            self.enqueue_commit_to_target_llc(actions, task);
        } else if self.published_target_llc {
            self.enqueue_commit_to_target_llc(actions, task);
        } else {
            self.push_dispatch(actions, task, DispatchTarget::RecoveryCpu);
        }
    }

    fn publish_mask_without_target_llc(&mut self, actions: &mut Vec<PolicyAction>) {
        self.published_target_llc = false;
        self.mask_generation += 1;
        self.push_event(
            actions,
            PolicyEventKind::PublishMask,
            None,
            Some(self.plan.control_cpu),
            None,
        );
    }

    fn update_observe_queue(&mut self, actions: &mut Vec<PolicyAction>) {
        if !self.published_target_llc && !self.target_q.is_empty() {
            self.drain_target_llc = true;
        }
        self.push_event(
            actions,
            PolicyEventKind::UpdateObserveQueue,
            None,
            Some(self.plan.control_cpu),
            None,
        );
    }

    fn enqueue_commit_to_target_llc(&mut self, actions: &mut Vec<PolicyAction>, task: TaskRef) {
        self.target_q.push(task);
        self.push_event(
            actions,
            PolicyEventKind::EnqueueCommit,
            Some(task),
            Some(self.plan.target_cpu),
            Some(self.plan.target_llc),
        );
    }

    fn on_dispatch_tick(&mut self, actions: &mut Vec<PolicyAction>) {
        if self.target_q.is_empty() {
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
            self.push_event(
                actions,
                PolicyEventKind::StableInvalidState,
                self.target_q.first().copied(),
                None,
                None,
            );
        }
    }

    fn on_recovery_deadline_elapsed(&mut self, actions: &mut Vec<PolicyAction>) {
        if !self.target_q.is_empty() && !self.published_target_llc && !self.drain_target_llc {
            self.drain_target_llc = true;
            self.push_event(
                actions,
                PolicyEventKind::RecoveryDrainEnabled,
                self.target_q.first().copied(),
                Some(self.plan.recovery_cpu),
                None,
            );
        }
    }

    fn pop_front_task(&mut self) -> TaskRef {
        self.target_q.remove(0)
    }

    fn push_event(
        &self,
        actions: &mut Vec<PolicyAction>,
        kind: PolicyEventKind,
        task: Option<TaskRef>,
        cpu: Option<CpuId>,
        selected_target_llc: Option<LlcId>,
    ) {
        actions.push(PolicyAction::Emit(PolicyEvent {
            kind,
            task,
            cpu,
            selected_target_llc,
            snapshot: self.snapshot(),
        }));
    }

    fn push_dispatch(
        &self,
        actions: &mut Vec<PolicyAction>,
        task: TaskRef,
        target: DispatchTarget,
    ) {
        actions.push(PolicyAction::Dispatch(DispatchAction {
            task,
            target,
            snapshot: self.snapshot(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> PolicyPlan {
        PolicyPlan {
            target_cpu: 0,
            target_llc: 0,
            recovery_cpu: 1,
            recovery_llc: 1,
            control_cpu: 2,
        }
    }

    fn task() -> TaskRef {
        TaskRef::new(100, 1)
    }

    #[test]
    fn first_deterministic_enqueue_exposes_stranded_state() {
        let mut policy = PolicyCore::new(plan(), PolicyMode::Deterministic);
        let actions = policy.step(PolicyInput::Enqueued {
            task: task(),
            is_problem_workload: true,
        });

        let kinds = actions
            .iter()
            .filter_map(|action| match action {
                PolicyAction::Emit(event) => Some(event.kind),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                PolicyEventKind::WorkloadMatched,
                PolicyEventKind::EnqueueSelect,
                PolicyEventKind::PublishMask,
                PolicyEventKind::UpdateObserveQueue,
                PolicyEventKind::EnqueueCommit,
            ]
        );
        assert!(actions.contains(&PolicyAction::HoldGate));
        assert_eq!(
            policy.snapshot(),
            PolicySnapshot {
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
        let mut policy = PolicyCore::new(plan(), PolicyMode::Deterministic);
        let _ = policy.step(PolicyInput::Enqueued {
            task: task(),
            is_problem_workload: true,
        });

        let invalid = policy.step(PolicyInput::DispatchTick);
        assert!(matches!(
            invalid.as_slice(),
            [PolicyAction::Emit(PolicyEvent {
                kind: PolicyEventKind::StableInvalidState,
                ..
            })]
        ));

        let recovery = policy.step(PolicyInput::RecoveryDeadlineElapsed);
        assert!(matches!(
            recovery.as_slice(),
            [PolicyAction::Emit(PolicyEvent {
                kind: PolicyEventKind::RecoveryDrainEnabled,
                ..
            })]
        ));

        let dispatch = policy.step(PolicyInput::DispatchTick);
        assert!(matches!(
            dispatch.as_slice(),
            [PolicyAction::Dispatch(DispatchAction {
                target: DispatchTarget::RecoveryCpu,
                ..
            })]
        ));
        assert!(policy.recovered());
        assert_eq!(policy.scheduled_len(), 0);
    }

    #[test]
    fn non_problem_task_bypasses_policy_queue() {
        let mut policy = PolicyCore::new(plan(), PolicyMode::Report);
        let actions = policy.step(PolicyInput::Enqueued {
            task: task(),
            is_problem_workload: false,
        });

        assert!(matches!(
            actions.as_slice(),
            [PolicyAction::Dispatch(DispatchAction {
                target: DispatchTarget::Any,
                ..
            })]
        ));
        assert_eq!(policy.scheduled_len(), 0);
    }
}
