use vstd::prelude::*;

verus! {

#[derive(Clone, Copy)]
pub struct PolicyCore {
    pub target_q: nat,
    pub published_target_llc: bool,
    pub drain_target_llc: bool,
    pub mask_generation: nat,
}

#[derive(Clone, Copy)]
pub enum EnqueuePc {
    Start,
    Selected,
    Inserted,
    Repaired,
}

#[derive(Clone, Copy)]
pub enum ConfigPc {
    Start,
    Published,
    Observed,
}

#[derive(Clone, Copy)]
pub enum DrainPc {
    Start,
    DisabledMaybe,
    Consumed,
    Repaired,
}

#[derive(Clone, Copy)]
pub struct InterleavingState {
    pub policy: PolicyCore,
    pub enqueue_pc: EnqueuePc,
    pub config_pc: ConfigPc,
    pub drain_pc: DrainPc,
    pub enqueue_selected_target_llc: bool,
    pub drain_disabled_local: bool,
}

#[derive(Clone, Copy)]
pub enum InterleavingStep {
    EnqueueSelect,
    EnqueueInsert,
    EnqueueRepair,
    ConfigPublishMask,
    ConfigObserveQueue,
    DrainMaybeDisable,
    DrainConsume,
    DrainRepair,
}

pub open spec fn bad_state(policy: PolicyCore) -> bool {
    policy.target_q > 0
        && !policy.published_target_llc
        && !policy.drain_target_llc
}

pub open spec fn no_lost_drain(policy: PolicyCore) -> bool {
    policy.target_q > 0 && !policy.published_target_llc ==> policy.drain_target_llc
}

pub open spec fn initial_policy(policy: PolicyCore) -> bool {
    policy.target_q == 0
        && policy.published_target_llc
        && !policy.drain_target_llc
        && policy.mask_generation == 0
}

pub open spec fn initial_interleaving_state(state: InterleavingState) -> bool {
    initial_policy(state.policy)
        && matches!(state.enqueue_pc, EnqueuePc::Start)
        && matches!(state.config_pc, ConfigPc::Start)
        && matches!(state.drain_pc, DrainPc::Start)
        && !state.enqueue_selected_target_llc
        && !state.drain_disabled_local
}

pub open spec fn stable_completed_state(state: InterleavingState) -> bool {
    matches!(state.enqueue_pc, EnqueuePc::Repaired)
        && matches!(state.config_pc, ConfigPc::Observed)
        && (matches!(state.drain_pc, DrainPc::Start) || matches!(state.drain_pc, DrainPc::Repaired))
}

pub open spec fn repair_owed(state: InterleavingState) -> bool {
    matches!(state.enqueue_pc, EnqueuePc::Inserted)
        || matches!(state.config_pc, ConfigPc::Published)
        || (
            state.drain_disabled_local
            && (
                matches!(state.drain_pc, DrainPc::DisabledMaybe)
                || matches!(state.drain_pc, DrainPc::Consumed)
            )
        )
}

pub open spec fn protocol_invariant(state: InterleavingState) -> bool {
    no_lost_drain(state.policy) || repair_owed(state)
}

pub open spec fn publish_mask_without_target_llc(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: false,
        drain_target_llc: policy.drain_target_llc,
        mask_generation: policy.mask_generation + 1,
    }
}

pub open spec fn config_observe_queue(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc
            || (!policy.published_target_llc && policy.target_q > 0),
        mask_generation: policy.mask_generation,
    }
}

pub open spec fn enqueue_insert_to_target_llc(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q + 1,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc,
        mask_generation: policy.mask_generation,
    }
}

pub open spec fn enqueue_repair_drain(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc || !policy.published_target_llc,
        mask_generation: policy.mask_generation,
    }
}

pub open spec fn drain_disable_if_likely_empty(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: if policy.target_q <= 1 { false } else { policy.drain_target_llc },
        mask_generation: policy.mask_generation,
    }
}

pub open spec fn drain_consume_one(policy: PolicyCore) -> PolicyCore {
    PolicyCore {
        target_q: if policy.target_q > 0 { (policy.target_q - 1) as nat } else { 0 },
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc,
        mask_generation: policy.mask_generation,
    }
}

pub open spec fn drain_repair_after_disable(policy: PolicyCore, disabled_local: bool) -> PolicyCore {
    PolicyCore {
        target_q: policy.target_q,
        published_target_llc: policy.published_target_llc,
        drain_target_llc: policy.drain_target_llc || (disabled_local && policy.target_q > 0),
        mask_generation: policy.mask_generation,
    }
}

pub open spec fn interleaving_step(state: InterleavingState, step: InterleavingStep) -> InterleavingState {
    match step {
        InterleavingStep::EnqueueSelect => {
            if matches!(state.enqueue_pc, EnqueuePc::Start) && state.policy.published_target_llc {
                InterleavingState {
                    policy: state.policy,
                    enqueue_pc: EnqueuePc::Selected,
                    config_pc: state.config_pc,
                    drain_pc: state.drain_pc,
                    enqueue_selected_target_llc: true,
                    drain_disabled_local: state.drain_disabled_local,
                }
            } else {
                state
            }
        },
        InterleavingStep::EnqueueInsert => {
            if matches!(state.enqueue_pc, EnqueuePc::Selected) && state.enqueue_selected_target_llc {
                InterleavingState {
                    policy: enqueue_insert_to_target_llc(state.policy),
                    enqueue_pc: EnqueuePc::Inserted,
                    config_pc: state.config_pc,
                    drain_pc: state.drain_pc,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                    drain_disabled_local: state.drain_disabled_local,
                }
            } else {
                state
            }
        },
        InterleavingStep::EnqueueRepair => {
            if matches!(state.enqueue_pc, EnqueuePc::Inserted) {
                InterleavingState {
                    policy: enqueue_repair_drain(state.policy),
                    enqueue_pc: EnqueuePc::Repaired,
                    config_pc: state.config_pc,
                    drain_pc: state.drain_pc,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                    drain_disabled_local: state.drain_disabled_local,
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
                    drain_pc: state.drain_pc,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                    drain_disabled_local: state.drain_disabled_local,
                }
            } else {
                state
            }
        },
        InterleavingStep::ConfigObserveQueue => {
            if matches!(state.config_pc, ConfigPc::Published) {
                InterleavingState {
                    policy: config_observe_queue(state.policy),
                    enqueue_pc: state.enqueue_pc,
                    config_pc: ConfigPc::Observed,
                    drain_pc: state.drain_pc,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                    drain_disabled_local: state.drain_disabled_local,
                }
            } else {
                state
            }
        },
        InterleavingStep::DrainMaybeDisable => {
            if matches!(state.drain_pc, DrainPc::Start) && state.policy.drain_target_llc {
                InterleavingState {
                    policy: drain_disable_if_likely_empty(state.policy),
                    enqueue_pc: state.enqueue_pc,
                    config_pc: state.config_pc,
                    drain_pc: DrainPc::DisabledMaybe,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                    drain_disabled_local: state.policy.target_q <= 1,
                }
            } else {
                state
            }
        },
        InterleavingStep::DrainConsume => {
            if matches!(state.drain_pc, DrainPc::DisabledMaybe) {
                InterleavingState {
                    policy: drain_consume_one(state.policy),
                    enqueue_pc: state.enqueue_pc,
                    config_pc: state.config_pc,
                    drain_pc: DrainPc::Consumed,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                    drain_disabled_local: state.drain_disabled_local,
                }
            } else {
                state
            }
        },
        InterleavingStep::DrainRepair => {
            if matches!(state.drain_pc, DrainPc::Consumed) {
                InterleavingState {
                    policy: drain_repair_after_disable(state.policy, state.drain_disabled_local),
                    enqueue_pc: state.enqueue_pc,
                    config_pc: state.config_pc,
                    drain_pc: DrainPc::Repaired,
                    enqueue_selected_target_llc: state.enqueue_selected_target_llc,
                    drain_disabled_local: state.drain_disabled_local,
                }
            } else {
                state
            }
        },
    }
}

pub proof fn initial_satisfies_invariant()
    ensures
        forall |s: InterleavingState| initial_interleaving_state(s) ==> protocol_invariant(s),
{
    assert(forall |s: InterleavingState| initial_interleaving_state(s) ==> protocol_invariant(s));
}

pub proof fn step_preserves_invariant()
    ensures
        forall |s: InterleavingState, step: InterleavingStep|
            protocol_invariant(s) ==> protocol_invariant(interleaving_step(s, step)),
{
    assert(forall |s: InterleavingState, step: InterleavingStep|
        protocol_invariant(s) ==> protocol_invariant(interleaving_step(s, step))
    );
}

pub proof fn stable_invariant_implies_no_bad_state()
    ensures
        forall |s: InterleavingState|
            protocol_invariant(s) && stable_completed_state(s) ==> !bad_state(s.policy),
{
    assert(forall |s: InterleavingState|
        protocol_invariant(s) && stable_completed_state(s) ==> !bad_state(s.policy)
    );
}

pub open spec fn run_steps(state: InterleavingState, steps: Seq<InterleavingStep>) -> InterleavingState
    decreases steps.len(),
{
    if steps.len() == 0 {
        state
    } else {
        run_steps(
            interleaving_step(state, steps[0]),
            steps.subrange(1, steps.len() as int),
        )
    }
}

pub proof fn run_steps_preserves_invariant(state: InterleavingState, steps: Seq<InterleavingStep>)
    requires
        protocol_invariant(state),
    ensures
        protocol_invariant(run_steps(state, steps)),
    decreases steps.len(),
{
    if steps.len() == 0 {
    } else {
        step_preserves_invariant();
        assert(protocol_invariant(interleaving_step(state, steps[0])));
        run_steps_preserves_invariant(
            interleaving_step(state, steps[0]),
            steps.subrange(1, steps.len() as int),
        );
    }
}

pub proof fn stable_trace_has_no_bad_state(initial: InterleavingState, steps: Seq<InterleavingStep>)
    requires
        initial_interleaving_state(initial),
        stable_completed_state(run_steps(initial, steps)),
    ensures
        !bad_state(run_steps(initial, steps).policy),
{
    initial_satisfies_invariant();
    assert(protocol_invariant(initial));
    run_steps_preserves_invariant(initial, steps);
    stable_invariant_implies_no_bad_state();
}

fn main() {
}

}
