use vstd::prelude::*;

verus! {

#[derive(Clone, Copy)]
pub struct TaskRef {
    pub pid: int,
    pub generation: nat,
}

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub mask_generation: nat,
    pub q: nat,
    pub c: bool,
    pub d: bool,
    pub selected_once: bool,
    pub recovered: bool,
}

#[derive(Clone, Copy)]
pub enum DispatchTarget {
    Any,
    TargetCpu,
    RecoveryCpu,
}

#[derive(Clone, Copy)]
pub enum PolicyInput {
    Enqueued {
        task: TaskRef,
        is_problem_workload: bool,
    },
    DispatchTick,
    RecoveryDeadlineElapsed,
}

#[derive(Clone, Copy)]
pub enum PolicyAction {
    HoldGate,
    StableInvalidState,
    Dispatch {
        task: TaskRef,
        target: DispatchTarget,
    },
}

#[derive(Clone, Copy)]
pub struct PolicyCore {
    pub target_q: nat,
    pub published_target_llc: bool,
    pub drain_target_llc: bool,
    pub mask_generation: nat,
    pub selected_once: bool,
    pub invalid_reported: bool,
    pub recovered: bool,
}

#[derive(Clone, Copy)]
pub enum EnqueuePc {
    Start,
    Selected,
    Committed,
}

#[derive(Clone, Copy)]
pub enum ConfigPc {
    Start,
    Published,
    Observed,
}

#[derive(Clone, Copy)]
pub struct InterleavingState {
    pub policy: PolicyCore,
    pub enqueue_pc: EnqueuePc,
    pub config_pc: ConfigPc,
    pub enqueue_selected_target_llc: bool,
}

#[derive(Clone, Copy)]
pub enum InterleavingStep {
    EnqueueSelect,
    ConfigPublishMask,
    ConfigObserveQueue,
    EnqueueCommit,
}

pub open spec fn bad_state(policy: PolicyCore) -> bool {
    policy.target_q > 0
        && !policy.published_target_llc
        && !policy.drain_target_llc
}

pub open spec fn initial_policy(policy: PolicyCore) -> bool {
    policy.target_q == 0
        && policy.published_target_llc
        && !policy.drain_target_llc
        && policy.mask_generation == 0
        && !policy.selected_once
        && !policy.invalid_reported
        && !policy.recovered
}

pub open spec fn initial_interleaving_state(state: InterleavingState) -> bool {
    initial_policy(state.policy)
        && matches!(state.enqueue_pc, EnqueuePc::Start)
        && matches!(state.config_pc, ConfigPc::Start)
        && !state.enqueue_selected_target_llc
}

pub open spec fn bad_interleaving_state(state: InterleavingState) -> bool {
    bad_state(state.policy)
        && matches!(state.enqueue_pc, EnqueuePc::Committed)
        && matches!(state.config_pc, ConfigPc::Observed)
}

pub open spec fn mark_selected_once(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc,
        mask_generation: policy.mask_generation,
        selected_once: true,
        invalid_reported: policy.invalid_reported,
        recovered: policy.recovered,
    }
}

pub open spec fn publish_mask_without_target_llc(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: false,
        drain_target_llc: policy.drain_target_llc,
        mask_generation: policy.mask_generation + 1,
        selected_once: policy.selected_once,
        invalid_reported: policy.invalid_reported,
        recovered: policy.recovered,
    }
}

pub open spec fn update_observe_queue(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc
            || (!policy.published_target_llc && policy.target_q > 0),
        mask_generation: policy.mask_generation,
        selected_once: policy.selected_once,
        invalid_reported: policy.invalid_reported,
        recovered: policy.recovered,
    }
}

pub open spec fn enqueue_commit_to_target_llc(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q + 1,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc,
        mask_generation: policy.mask_generation,
        selected_once: policy.selected_once,
        invalid_reported: policy.invalid_reported,
        recovered: policy.recovered,
    }
}

pub open spec fn interleaving_step(state: InterleavingState, step: InterleavingStep) -> InterleavingState {
    match step {
        InterleavingStep::EnqueueSelect => {
            if matches!(state.enqueue_pc, EnqueuePc::Start) && state.policy.published_target_llc {
                InterleavingState {
                    policy: mark_selected_once(state.policy),
                    enqueue_pc: EnqueuePc::Selected,
                    config_pc: state.config_pc,
                    enqueue_selected_target_llc: true,
                }
            } else {
                state
            }
        },
        InterleavingStep::ConfigPublishMask => {
            if matches!(state.config_pc, ConfigPc::Start) {
                InterleavingState {
                    policy: publish_mask_without_target_llc(state.policy),
                    enqueue_pc: state.enqueue_pc,
                    config_pc: ConfigPc::Published,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                }
            } else {
                state
            }
        },
        InterleavingStep::ConfigObserveQueue => {
            if matches!(state.config_pc, ConfigPc::Published) {
                InterleavingState {
                    policy: update_observe_queue(state.policy),
                    enqueue_pc: state.enqueue_pc,
                    config_pc: ConfigPc::Observed,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                }
            } else {
                state
            }
        },
        InterleavingStep::EnqueueCommit => {
            if matches!(state.enqueue_pc, EnqueuePc::Selected) && state.enqueue_selected_target_llc {
                InterleavingState {
                    policy: enqueue_commit_to_target_llc(state.policy),
                    enqueue_pc: EnqueuePc::Committed,
                    config_pc: state.config_pc,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                }
            } else {
                state
            }
        },
    }
}

pub proof fn concurrent_interleaving_reaches_bad_state_witness()
    ensures
        exists |s: InterleavingState|
            initial_interleaving_state(s)
            && bad_interleaving_state(
                interleaving_step(
                    interleaving_step(
                        interleaving_step(
                            interleaving_step(s, InterleavingStep::EnqueueSelect),
                            InterleavingStep::ConfigPublishMask
                        ),
                        InterleavingStep::ConfigObserveQueue
                    ),
                    InterleavingStep::EnqueueCommit
                )
            ),
{
    let initial = InterleavingState {
        policy: PolicyCore {
            target_q: 0,
            published_target_llc: true,
            drain_target_llc: false,
            mask_generation: 0,
            selected_once: false,
            invalid_reported: false,
            recovered: false,
        },
        enqueue_pc: EnqueuePc::Start,
        config_pc: ConfigPc::Start,
        enqueue_selected_target_llc: false,
    };
    let after_select = interleaving_step(initial, InterleavingStep::EnqueueSelect);
    let after_publish = interleaving_step(after_select, InterleavingStep::ConfigPublishMask);
    let after_observe = interleaving_step(after_publish, InterleavingStep::ConfigObserveQueue);
    let after_commit = interleaving_step(after_observe, InterleavingStep::EnqueueCommit);

    assert(initial_interleaving_state(initial));
    assert(matches!(after_select.enqueue_pc, EnqueuePc::Selected));
    assert(after_select.enqueue_selected_target_llc);
    assert(after_select.policy.selected_once);

    assert(matches!(after_publish.config_pc, ConfigPc::Published));
    assert(after_publish.policy.target_q == 0);
    assert(!after_publish.policy.published_target_llc);
    assert(after_publish.policy.mask_generation == 1);

    assert(matches!(after_observe.config_pc, ConfigPc::Observed));
    assert(after_observe.policy.target_q == 0);
    assert(!after_observe.policy.drain_target_llc);

    assert(matches!(after_commit.enqueue_pc, EnqueuePc::Committed));
    assert(after_commit.policy.target_q == 1);
    assert(!after_commit.policy.published_target_llc);
    assert(!after_commit.policy.drain_target_llc);
    assert(bad_interleaving_state(after_commit));

    assert(exists |s: InterleavingState|
        initial_interleaving_state(s)
        && bad_interleaving_state(
            interleaving_step(
                interleaving_step(
                    interleaving_step(
                        interleaving_step(s, InterleavingStep::EnqueueSelect),
                        InterleavingStep::ConfigPublishMask
                    ),
                    InterleavingStep::ConfigObserveQueue
                ),
                InterleavingStep::EnqueueCommit
            )
        )
    );
}

fn main() {
}

}
