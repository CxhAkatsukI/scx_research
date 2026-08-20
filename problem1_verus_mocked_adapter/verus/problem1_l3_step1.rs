use vstd::prelude::*;

verus! {

#[derive(Clone, Copy)]
pub enum RunMode {
    Deterministic,
}

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
    pub mode: RunMode,
    pub target_q: nat,
    pub published_target_llc: bool,
    pub drain_target_llc: bool,
    pub mask_generation: nat,
    pub selected_once: bool,
    pub invalid_reported: bool,
    pub recovered: bool,
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

pub open spec fn mark_selected_once(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        mode: policy.mode,
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
        mode: policy.mode,
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
        mode: policy.mode,
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
        mode: policy.mode,
        target_q: policy.target_q + 1,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc,
        mask_generation: policy.mask_generation,
        selected_once: policy.selected_once,
        invalid_reported: policy.invalid_reported,
        recovered: policy.recovered,
    }
}

pub open spec fn first_problem_enqueue_step(policy: PolicyCore) -> PolicyCore {
    enqueue_commit_to_target_llc(
        update_observe_queue(
            publish_mask_without_target_llc(
                mark_selected_once(policy)
            )
        )
    )
}

pub open spec fn step(policy: PolicyCore, input: PolicyInput) -> PolicyCore {
    match input {
        PolicyInput::Enqueued { task: _, is_problem_workload } => {
            if is_problem_workload && !policy.selected_once {
                first_problem_enqueue_step(policy)
            } else {
                policy
            }
        },
        PolicyInput::DispatchTick => policy,
        PolicyInput::RecoveryDeadlineElapsed => policy,
    }
}

pub proof fn one_enqueue_reaches_bad_state_witness()
    ensures
        exists |s: PolicyCore, input: PolicyInput| initial_policy(s) && bad_state(step(s, input)),
{
    let initial = PolicyCore {
        mode: RunMode::Deterministic,
        target_q: 0,
        published_target_llc: true,
        drain_target_llc: false,
        mask_generation: 0,
        selected_once: false,
        invalid_reported: false,
        recovered: false,
    };
    let input = PolicyInput::Enqueued {
        task: TaskRef {
            pid: 100,
            generation: 1,
        },
        is_problem_workload: true,
    };
    let after_select = mark_selected_once(initial);
    let after_publish = publish_mask_without_target_llc(after_select);
    let after_observe = update_observe_queue(after_publish);
    let after_commit = enqueue_commit_to_target_llc(after_observe);
    let after = step(initial, input);

    assert(initial_policy(initial));
    assert(after_select.selected_once);
    assert(after_publish.target_q == 0);
    assert(!after_publish.published_target_llc);
    assert(after_publish.mask_generation == 1);
    assert(after_observe.target_q == 0);
    assert(!after_observe.drain_target_llc);
    assert(after_commit.target_q == 1);
    assert(!after_commit.published_target_llc);
    assert(!after_commit.drain_target_llc);
    assert(after == after_commit);
    assert(bad_state(after));
    assert(exists |s: PolicyCore, input: PolicyInput| initial_policy(s) && bad_state(step(s, input)));
}

fn main() {
}

}
