use crate::protocol::{EnqueueTicket, ProtocolState, Task, TaskId, PARTITION_A};
use crate::topology::{discover_or_example, TopologyPlan};
use crate::trace::{EventSpec, TraceEvent};

pub const WORKLOAD_TASK: TaskId = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunMode {
    Report,
    Deterministic,
    Stochastic,
}

impl RunMode {
    pub fn parse(input: &str) -> Result<Self, String> {
        match input {
            "report" => Ok(Self::Report),
            "deterministic" => Ok(Self::Deterministic),
            "stochastic" => Ok(Self::Stochastic),
            _ => Err(format!(
                "unknown mode `{input}`; expected report, deterministic, or stochastic"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Report => "report_model",
            Self::Deterministic => "deterministic_model",
            Self::Stochastic => "stochastic_model",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessConfig {
    pub mode: RunMode,
    pub attempts: usize,
    pub use_synthetic_topology: bool,
}

impl HarnessConfig {
    pub fn new(mode: RunMode) -> Self {
        Self {
            mode,
            attempts: 64,
            use_synthetic_topology: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessSummary {
    pub mode: RunMode,
    pub attempts: usize,
    pub invalid_hits: usize,
    pub recovered: bool,
    pub synthetic_topology: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRun {
    pub events: Vec<TraceEvent>,
    pub summary: HarnessSummary,
}

pub fn run_local(config: &HarnessConfig) -> HarnessRun {
    let plan = if config.use_synthetic_topology {
        TopologyPlan::example()
    } else {
        discover_or_example()
    };

    match config.mode {
        RunMode::Report => run_report(&plan),
        RunMode::Deterministic => run_deterministic(&plan),
        RunMode::Stochastic => run_stochastic(&plan, config.attempts),
    }
}

fn run_report(plan: &TopologyPlan) -> HarnessRun {
    let mut recorder = Recorder::new(RunMode::Report.as_str());
    let mut state = make_state(plan);

    record_topology(&mut recorder, &state, plan);

    let ticket = enqueue_select(
        &mut recorder,
        &mut state,
        plan,
        "report observes the enqueue-side selection without holding the window",
    );
    publish_mask(&mut recorder, &mut state, plan);
    update_observe_queue(&mut recorder, &mut state, plan);
    recorder.push(
        &state,
        EventSpec::model(
            recorder.mode,
            "window_reported",
            plan.target_llc,
            "dry-run report mode observed the vulnerable window; real report mode must not hold it",
        )
        .with_source("dry_run_harness")
        .with_cpu(plan.control_cpu)
        .with_task(WORKLOAD_TASK),
    );
    enqueue_commit(&mut recorder, &mut state, plan, ticket);
    let invalid = record_stable_state(&mut recorder, &state, plan);
    let recovered = recover(&mut recorder, &mut state, plan);

    finish(
        recorder,
        RunMode::Report,
        1,
        usize::from(invalid),
        recovered,
        plan,
    )
}

fn run_deterministic(plan: &TopologyPlan) -> HarnessRun {
    let mut recorder = Recorder::new(RunMode::Deterministic.as_str());
    let mut state = make_state(plan);

    record_topology(&mut recorder, &state, plan);

    let ticket = enqueue_select(
        &mut recorder,
        &mut state,
        plan,
        "deterministic mode arms the bounded gate after a real enqueue-side selection",
    );
    recorder.push(
        &state,
        EventSpec::model(
            recorder.mode,
            "gate_armed",
            plan.target_llc,
            "dry-run gate marks the legal window; real adapter must bound the hold and must not write Q/C/D",
        )
        .with_source("dry_run_harness")
        .with_cpu(plan.target_cpu)
        .with_task(WORKLOAD_TASK)
        .with_selected_target_llc(ticket.target_llc),
    );
    publish_mask(&mut recorder, &mut state, plan);
    update_observe_queue(&mut recorder, &mut state, plan);
    recorder.push(
        &state,
        EventSpec::model(
            recorder.mode,
            "gate_released",
            plan.target_llc,
            "dry-run releases the modeled enqueue; real adapter should release before its bounded deadline",
        )
        .with_source("dry_run_harness")
        .with_cpu(plan.control_cpu)
        .with_task(WORKLOAD_TASK)
        .with_selected_target_llc(ticket.target_llc),
    );
    enqueue_commit(&mut recorder, &mut state, plan, ticket);
    let invalid = record_stable_state(&mut recorder, &state, plan);
    let recovered = recover(&mut recorder, &mut state, plan);

    finish(
        recorder,
        RunMode::Deterministic,
        1,
        usize::from(invalid),
        recovered,
        plan,
    )
}

fn run_stochastic(plan: &TopologyPlan, attempts: usize) -> HarnessRun {
    let attempts = attempts.max(1);
    let mut representative = None;
    let mut invalid_hits = 0;

    for attempt in 0..attempts {
        let ordering = attempt % 3;
        let mut recorder = Recorder::new(RunMode::Stochastic.as_str());
        let mut state = make_state(plan);

        record_topology(&mut recorder, &state, plan);
        recorder.push(
            &state,
            EventSpec::model(
                recorder.mode,
                "stochastic_attempt",
                plan.target_llc,
                "dry-run stochastic mode enumerates natural interleavings without holding the window",
            )
            .with_source("dry_run_harness")
            .with_cpu(plan.control_cpu)
            .with_task(WORKLOAD_TASK),
        );

        let invalid = match ordering {
            0 => run_bad_interleaving(&mut recorder, &mut state, plan),
            1 => run_enqueue_first_interleaving(&mut recorder, &mut state, plan),
            _ => run_update_first_interleaving(&mut recorder, &mut state, plan),
        };

        if invalid {
            invalid_hits += 1;
        }

        if representative.is_none() || invalid {
            representative = Some((recorder, state, invalid, attempt));
        }
    }

    let (mut recorder, state, representative_invalid, attempt) =
        representative.expect("attempts is forced to be at least one");
    recorder.push(
        &state,
        EventSpec::model(
            recorder.mode,
            "stochastic_summary",
            plan.target_llc,
            &format!(
                "attempts={attempts}, invalid_hits={invalid_hits}, representative_attempt={attempt}, representative_invalid={representative_invalid}"
            ),
        )
        .with_source("dry_run_harness")
        .with_cpu(plan.control_cpu)
        .with_task(WORKLOAD_TASK),
    );

    finish(
        recorder,
        RunMode::Stochastic,
        attempts,
        invalid_hits,
        false,
        plan,
    )
}

fn run_bad_interleaving(
    recorder: &mut Recorder,
    state: &mut ProtocolState,
    plan: &TopologyPlan,
) -> bool {
    let ticket = enqueue_select(
        recorder,
        state,
        plan,
        "stochastic representative picked the bad enqueue/update interleaving",
    );
    publish_mask(recorder, state, plan);
    update_observe_queue(recorder, state, plan);
    enqueue_commit(recorder, state, plan, ticket);
    record_stable_state(recorder, state, plan)
}

fn run_enqueue_first_interleaving(
    recorder: &mut Recorder,
    state: &mut ProtocolState,
    plan: &TopologyPlan,
) -> bool {
    let ticket = enqueue_select(
        recorder,
        state,
        plan,
        "stochastic representative picked the safe enqueue-first interleaving",
    );
    enqueue_commit(recorder, state, plan, ticket);
    publish_mask(recorder, state, plan);
    update_observe_queue(recorder, state, plan);
    record_stable_state(recorder, state, plan)
}

fn run_update_first_interleaving(
    recorder: &mut Recorder,
    state: &mut ProtocolState,
    plan: &TopologyPlan,
) -> bool {
    publish_mask(recorder, state, plan);
    update_observe_queue(recorder, state, plan);
    let ticket = enqueue_select(
        recorder,
        state,
        plan,
        "stochastic representative picked the safe update-first interleaving",
    );
    enqueue_commit(recorder, state, plan, ticket);
    record_stable_state(recorder, state, plan)
}

fn make_state(plan: &TopologyPlan) -> ProtocolState {
    ProtocolState::new(
        PARTITION_A,
        plan.topology.clone(),
        plan.initial_partition_cpus(),
        [Task::new(WORKLOAD_TASK, plan.workload_cpus())],
    )
}

fn record_topology(recorder: &mut Recorder, state: &ProtocolState, plan: &TopologyPlan) {
    let source = if plan.synthetic {
        "synthetic_topology"
    } else {
        "sysfs_topology"
    };
    let mut note = format!(
        "target=CPU{}/LLC{}, recovery=CPU{}/LLC{}, control=CPU{}",
        plan.target_cpu, plan.target_llc, plan.recovery_cpu, plan.recovery_llc, plan.control_cpu
    );
    if !plan.notes.is_empty() {
        note.push_str("; ");
        note.push_str(&plan.notes.join("; "));
    }

    recorder.push(
        state,
        EventSpec::model(recorder.mode, "topology_plan", plan.target_llc, &note)
            .with_source(source)
            .with_cpu(plan.control_cpu)
            .with_task(WORKLOAD_TASK),
    );
}

fn enqueue_select(
    recorder: &mut Recorder,
    state: &mut ProtocolState,
    plan: &TopologyPlan,
    note: &str,
) -> EnqueueTicket {
    let ticket = state.enqueue_select(WORKLOAD_TASK, plan.target_llc);
    recorder.push(
        state,
        EventSpec::model(recorder.mode, "enqueue_select", ticket.target_llc, note)
            .with_source("protocol_model")
            .with_cpu(plan.target_cpu)
            .with_task(WORKLOAD_TASK)
            .with_selected_target_llc(ticket.target_llc),
    );
    ticket
}

fn publish_mask(recorder: &mut Recorder, state: &mut ProtocolState, plan: &TopologyPlan) {
    state.publish_mask(plan.updated_partition_cpus());
    recorder.push(
        state,
        EventSpec::model(
            recorder.mode,
            "publish_mask",
            plan.target_llc,
            "partition mask now excludes every CPU in the target LLC",
        )
        .with_source("protocol_model")
        .with_cpu(plan.control_cpu)
        .with_task(WORKLOAD_TASK),
    );
}

fn update_observe_queue(recorder: &mut Recorder, state: &mut ProtocolState, plan: &TopologyPlan) {
    state.update_observe_queue(plan.target_llc);
    recorder.push(
        state,
        EventSpec::model(
            recorder.mode,
            "update_observe_queue",
            plan.target_llc,
            "updater enables D only if it observes Q>0 after removing the target LLC",
        )
        .with_source("protocol_model")
        .with_cpu(plan.control_cpu)
        .with_task(WORKLOAD_TASK),
    );
}

fn enqueue_commit(
    recorder: &mut Recorder,
    state: &mut ProtocolState,
    plan: &TopologyPlan,
    ticket: EnqueueTicket,
) {
    let target_llc = ticket.target_llc;
    state.enqueue_commit(ticket);
    recorder.push(
        state,
        EventSpec::model(
            recorder.mode,
            "enqueue_commit",
            plan.target_llc,
            "enqueue commits the target chosen during enqueue_select",
        )
        .with_source("protocol_model")
        .with_cpu(plan.target_cpu)
        .with_task(WORKLOAD_TASK)
        .with_selected_target_llc(target_llc),
    );
}

fn record_stable_state(
    recorder: &mut Recorder,
    state: &ProtocolState,
    plan: &TopologyPlan,
) -> bool {
    let invalid = state.invalid_stable_stranding(plan.target_llc);
    let event = if invalid {
        "stable_invalid_state"
    } else {
        "stable_safe_state"
    };
    let note = if invalid {
        "both operations returned with Q>0, C=false, D=false, and workload remains eligible outside the orphan LLC"
    } else {
        "the chosen interleaving did not leave a stable stranded queue"
    };

    recorder.push(
        state,
        EventSpec::model(recorder.mode, event, plan.target_llc, note)
            .with_source("protocol_model")
            .with_task(WORKLOAD_TASK),
    );
    invalid
}

fn recover(recorder: &mut Recorder, state: &mut ProtocolState, plan: &TopologyPlan) -> bool {
    let drained = state.force_drain_for_test_recovery(plan.target_llc);
    recorder.push(
        state,
        EventSpec::model(
            recorder.mode,
            "recovery_drain_enabled",
            plan.target_llc,
            &format!("recovery enabled D and drained tasks {drained:?}"),
        )
        .with_source("protocol_model")
        .with_cpu(plan.recovery_cpu)
        .with_task(WORKLOAD_TASK),
    );

    let recovered = drained.contains(&WORKLOAD_TASK);
    if recovered {
        state.record_task_progress(WORKLOAD_TASK, 1);
    }

    recorder.push(
        state,
        EventSpec::model(
            recorder.mode,
            "recovered_task_progress",
            plan.target_llc,
            "task progress increases only after recovery",
        )
        .with_source("dry_run_harness")
        .with_cpu(plan.recovery_cpu)
        .with_task(WORKLOAD_TASK),
    );
    recovered
}

fn finish(
    recorder: Recorder,
    mode: RunMode,
    attempts: usize,
    invalid_hits: usize,
    recovered: bool,
    plan: &TopologyPlan,
) -> HarnessRun {
    HarnessRun {
        events: recorder.events,
        summary: HarnessSummary {
            mode,
            attempts,
            invalid_hits,
            recovered,
            synthetic_topology: plan.synthetic,
            notes: plan.notes.clone(),
        },
    }
}

struct Recorder {
    mode: &'static str,
    events: Vec<TraceEvent>,
    seq: u64,
}

impl Recorder {
    fn new(mode: &'static str) -> Self {
        Self {
            mode,
            events: Vec::new(),
            seq: 0,
        }
    }

    fn push(&mut self, state: &ProtocolState, spec: EventSpec<'_>) {
        let spec = spec.with_timestamp_ns(self.seq * 1_000);
        self.events
            .push(TraceEvent::from_state(self.seq, state, spec));
        self.seq += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{CPU0, CPU1, CPU2, LLC0};

    fn synthetic_config(mode: RunMode) -> HarnessConfig {
        let mut config = HarnessConfig::new(mode);
        config.use_synthetic_topology = true;
        config
    }

    #[test]
    fn deterministic_mode_reaches_and_recovers_invalid_state() {
        let run = run_local(&synthetic_config(RunMode::Deterministic));

        assert_eq!(run.summary.invalid_hits, 1);
        assert!(run.summary.recovered);
        assert!(run
            .events
            .iter()
            .any(|event| event.event == "stable_invalid_state"));
        assert!(run
            .events
            .iter()
            .any(|event| event.event == "recovered_task_progress" && event.task_progress == 1));
        assert!(run.events.iter().all(|event| !event.adapter_observed));
    }

    #[test]
    fn report_mode_marks_window_without_adapter_evidence() {
        let run = run_local(&synthetic_config(RunMode::Report));

        assert!(run
            .events
            .iter()
            .any(|event| event.event == "window_reported"));
        assert!(run.events.iter().all(|event| event.adapter_q.is_none()));
    }

    #[test]
    fn stochastic_mode_reports_attempts_and_hits() {
        let mut config = synthetic_config(RunMode::Stochastic);
        config.attempts = 6;
        let run = run_local(&config);

        assert_eq!(run.summary.attempts, 6);
        assert_eq!(run.summary.invalid_hits, 2);
        assert!(run
            .events
            .iter()
            .any(|event| event.event == "stochastic_summary"));
    }

    #[test]
    fn update_first_interleaving_avoids_target_llc() {
        let plan = TopologyPlan::example();
        let mut recorder = Recorder::new(RunMode::Stochastic.as_str());
        let mut state = make_state(&plan);

        assert!(!run_update_first_interleaving(
            &mut recorder,
            &mut state,
            &plan
        ));
        assert_eq!(state.queue_len(LLC0), 0);
        assert_eq!(state.queue_len(crate::protocol::LLC1), 1);
    }

    #[test]
    fn exported_cpu_constants_still_match_the_synthetic_plan() {
        let plan = TopologyPlan::example();

        assert_eq!(plan.target_cpu, CPU0);
        assert_eq!(plan.recovery_cpu, CPU1);
        assert_eq!(plan.control_cpu, CPU2);
    }
}
