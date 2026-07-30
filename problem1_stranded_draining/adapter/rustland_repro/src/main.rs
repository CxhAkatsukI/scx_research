// SPDX-License-Identifier: GPL-2.0

mod bpf_skel {
    include!(concat!(env!("OUT_DIR"), "/bpf_skel.rs"));
}
pub use bpf_skel::*;
pub mod bpf_intf {
    include!(concat!(env!("OUT_DIR"), "/bpf_intf.rs"));
}

#[rustfmt::skip]
mod bpf;

use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use bpf::{BpfScheduler, DispatchedTask, QueuedTask, RL_CPU_ANY};
use libbpf_rs::OpenObject;
use problem1_stranded_draining::harness::RunMode;
use problem1_stranded_draining::policy_core::{
    DispatchAction, DispatchTarget, PolicyAction, PolicyCore, PolicyEvent, PolicyEventKind,
    PolicyInput, PolicyMode, PolicyPlan, PolicySnapshot, TaskRef,
};
use problem1_stranded_draining::topology::{discover_or_example, TopologyPlan};
use scx_utils::libbpf_clap_opts::LibbpfOpts;

const SLICE_NS: u64 = 50_000;
const SCHED_NAME: &str = "p1_strand";
const WORKLOAD_COMM: &str = "problem1_workload";
const WORKLOAD_COMM_TRUNCATED: &str = "problem1_worklo";

fn main() -> Result<()> {
    let opts = Opts::parse(std::env::args().skip(1))?;
    let mut open_object = MaybeUninit::uninit();
    let mut scheduler = Scheduler::init(&opts, &mut open_object)?;
    scheduler.run()
}

#[derive(Clone, Debug)]
struct Opts {
    mode: RunMode,
    workload_pid: Option<i32>,
    gate_hold_ms: u64,
    recovery_delay_ms: u64,
    max_runtime_ms: u64,
    debug: bool,
}

impl Opts {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut mode = RunMode::Deterministic;
        let mut workload_pid = None;
        let mut gate_hold_ms = 5;
        let mut recovery_delay_ms = 100;
        let mut max_runtime_ms = 5_000;
        let mut debug = false;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--mode" => {
                    let value = next_value(&mut args, "--mode")?;
                    mode = RunMode::parse(&value).map_err(anyhow::Error::msg)?;
                }
                "--workload-pid" => {
                    let value = next_value(&mut args, "--workload-pid")?;
                    workload_pid = Some(value.parse()?);
                }
                "--gate-hold-ms" => {
                    let value = next_value(&mut args, "--gate-hold-ms")?;
                    gate_hold_ms = value.parse()?;
                }
                "--recovery-delay-ms" => {
                    let value = next_value(&mut args, "--recovery-delay-ms")?;
                    recovery_delay_ms = value.parse()?;
                }
                "--max-runtime-ms" => {
                    let value = next_value(&mut args, "--max-runtime-ms")?;
                    max_runtime_ms = value.parse()?;
                }
                "--debug" => debug = true,
                "--help" | "-h" => {
                    println!("{}", Self::usage());
                    std::process::exit(0);
                }
                _ => bail!("unknown argument `{arg}`\n{}", Self::usage()),
            }
        }

        Ok(Self {
            mode,
            workload_pid,
            gate_hold_ms,
            recovery_delay_ms,
            max_runtime_ms,
            debug,
        })
    }

    fn usage() -> &'static str {
        "usage: problem1_stranded_draining_rustland [--mode report|deterministic|stochastic] [--workload-pid PID] [--gate-hold-ms N] [--recovery-delay-ms N] [--max-runtime-ms N] [--debug]"
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next()
        .ok_or_else(|| anyhow::Error::msg(format!("{flag} requires a value")))
}

struct Scheduler<'a> {
    bpf: BpfScheduler<'a>,
    opts: Opts,
    plan: TopologyPlan,
    policy: PolicyCore,
    tasks: HashMap<TaskRef, QueuedTask>,
    invalid_since: Option<Instant>,
    started_at: Instant,
}

impl<'a> Scheduler<'a> {
    fn init(opts: &Opts, open_object: &'a mut MaybeUninit<OpenObject>) -> Result<Self> {
        let plan = discover_or_example();
        if plan.synthetic {
            bail!(
                "safe real adapter run needs a real sysfs topology with at least two LLCs; dry-run harness may use synthetic topology"
            );
        }

        emit_static_event(
            opts.mode,
            "topology_plan",
            &plan,
            None,
            Some(plan.control_cpu),
            None,
            0,
            0,
            true,
            false,
            "selected real sysfs topology for the rustland adapter",
        );
        emit_static_event(
            opts.mode,
            "load_requested",
            &plan,
            None,
            Some(plan.control_cpu),
            None,
            0,
            0,
            true,
            false,
            "loading rustland backend with partial switching enabled",
        );

        let open_opts = LibbpfOpts::default().into_bpf_open_opts();
        let bpf = BpfScheduler::init(
            open_object,
            open_opts,
            0,
            true,
            opts.debug,
            false,
            false,
            SLICE_NS,
            SCHED_NAME,
        )?;

        Ok(Self {
            bpf,
            opts: opts.clone(),
            policy: PolicyCore::new(policy_plan(&plan), policy_mode(opts.mode)),
            plan,
            tasks: HashMap::new(),
            invalid_since: None,
            started_at: Instant::now(),
        })
    }

    fn run(&mut self) -> Result<()> {
        let mut reported_dequeue_error = false;

        while !self.bpf.exited() && self.started_at.elapsed() < self.max_runtime() {
            loop {
                match self.bpf.dequeue_task() {
                    Ok(Some(task)) => self.handle_queued_task(task)?,
                    Ok(None) => break,
                    Err(errno) => {
                        if !reported_dequeue_error {
                            let note = format!("dequeue_task returned errno={errno}");
                            self.emit_snapshot_event(
                                "dequeue_error",
                                None,
                                Some(self.plan.control_cpu),
                                None,
                                self.policy.snapshot(),
                                &note,
                            );
                            reported_dequeue_error = true;
                        }
                        break;
                    }
                }
            }

            let actions = self.policy.step(PolicyInput::DispatchTick);
            self.apply_actions(actions)?;

            if self.invalid_since.is_some_and(|since| {
                since.elapsed() >= Duration::from_millis(self.opts.recovery_delay_ms)
            }) {
                let actions = self.policy.step(PolicyInput::RecoveryDeadlineElapsed);
                self.apply_actions(actions)?;
            }

            self.bpf.notify_complete(self.policy.scheduled_len() as u64);

            if self.policy.recovered() {
                break;
            }
        }

        let exited = self.bpf.exited();
        let event = if self.policy.recovered() {
            "adapter_summary_recovered"
        } else if exited {
            "adapter_summary_exited"
        } else {
            "adapter_summary_timeout"
        };
        self.emit_summary(event);

        let exit = self.bpf.shutdown_and_report()?;
        eprintln!("scheduler exit: {:?}", exit);
        Ok(())
    }

    fn max_runtime(&self) -> Duration {
        Duration::from_millis(self.opts.max_runtime_ms)
    }

    fn handle_queued_task(&mut self, task: QueuedTask) -> Result<()> {
        let task_ref = TaskRef::new(task.pid, task.enq_cnt);
        let is_problem_workload = self.is_problem_workload(&task);
        self.tasks.insert(task_ref, task);
        let actions = self.policy.step(PolicyInput::Enqueued {
            task: task_ref,
            is_problem_workload,
        });
        self.apply_actions(actions)
    }

    fn is_problem_workload(&self, task: &QueuedTask) -> bool {
        if let Some(pid) = self.opts.workload_pid {
            return task.pid == pid;
        }

        let comm = task.comm_str();
        comm == WORKLOAD_COMM
            || comm == WORKLOAD_COMM_TRUNCATED
            || comm.starts_with(WORKLOAD_COMM_TRUNCATED)
    }

    fn apply_actions(&mut self, actions: Vec<PolicyAction>) -> Result<()> {
        for action in actions {
            match action {
                PolicyAction::Emit(event) => self.emit_policy_event(event),
                PolicyAction::HoldGate => {
                    thread::sleep(Duration::from_millis(self.opts.gate_hold_ms));
                }
                PolicyAction::Dispatch(dispatch) => self.dispatch_policy_action(dispatch)?,
            }
        }
        Ok(())
    }

    fn dispatch_policy_action(&mut self, dispatch: DispatchAction) -> Result<()> {
        let task = self.take_task(dispatch.task)?;
        match dispatch.target {
            DispatchTarget::Any => self.dispatch_any(task),
            DispatchTarget::TargetCpu => self.dispatch_to_target(task, dispatch.snapshot),
            DispatchTarget::RecoveryCpu => self.dispatch_to_recovery(task, dispatch.snapshot),
        }
    }

    fn take_task(&mut self, task_ref: TaskRef) -> Result<QueuedTask> {
        self.tasks.remove(&task_ref).ok_or_else(|| {
            anyhow::Error::msg(format!(
                "policy asked to dispatch missing task pid={} enq_cnt={}",
                task_ref.pid, task_ref.enqueue_seq
            ))
        })
    }

    fn dispatch_any(&mut self, task: QueuedTask) -> Result<()> {
        let mut dispatched = DispatchedTask::new(&task);
        dispatched.cpu = RL_CPU_ANY;
        dispatched.slice_ns = SLICE_NS;
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }

    fn dispatch_to_target(&mut self, task: QueuedTask, snapshot: PolicySnapshot) -> Result<()> {
        let mut dispatched = DispatchedTask::new(&task);
        dispatched.cpu = self.plan.target_cpu as i32;
        dispatched.slice_ns = SLICE_NS;
        self.emit_snapshot_event(
            "dispatch_target",
            Some(task.pid),
            Some(self.plan.target_cpu),
            Some(self.plan.target_llc),
            snapshot,
            "dispatching on the target CPU while the target LLC is still published",
        );
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }

    fn dispatch_to_recovery(&mut self, task: QueuedTask, snapshot: PolicySnapshot) -> Result<()> {
        let mut dispatched = DispatchedTask::new(&task);
        dispatched.cpu = self.plan.recovery_cpu as i32;
        dispatched.slice_ns = SLICE_NS;
        self.emit_snapshot_event(
            "dispatch_recovery",
            Some(task.pid),
            Some(self.plan.recovery_cpu),
            Some(self.plan.recovery_llc),
            snapshot,
            "dispatching through the recovery CPU after D becomes true",
        );
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }

    fn emit_policy_event(&mut self, event: PolicyEvent) {
        if event.kind == PolicyEventKind::StableInvalidState && self.invalid_since.is_none() {
            self.invalid_since = Some(Instant::now());
        }

        let note = self.policy_event_note(event);
        self.emit_snapshot_event(
            event.kind.as_str(),
            event.task.map(|task| task.pid),
            event.cpu,
            event.selected_target_llc,
            event.snapshot,
            &note,
        );
    }

    fn policy_event_note(&self, event: PolicyEvent) -> String {
        match event.kind {
            PolicyEventKind::WorkloadMatched => {
                let (pid, comm) = event
                    .task
                    .and_then(|task| self.tasks.get(&task).map(|queued| (task.pid, queued.comm_str())))
                    .unwrap_or((event.task.map_or(-1, |task| task.pid), "unknown".to_string()));
                format!("matched workload pid={pid} comm={comm}")
            }
            PolicyEventKind::EnqueueSelect => {
                "enqueue observed the old mask and selected the target LLC".to_string()
            }
            PolicyEventKind::PublishMask => {
                "partition mask now excludes every CPU in the target LLC".to_string()
            }
            PolicyEventKind::UpdateObserveQueue => {
                "updater enables D only when it observes Q>0".to_string()
            }
            PolicyEventKind::EnqueueCommit => {
                "enqueue committed the LLC selected before the mask update".to_string()
            }
            PolicyEventKind::StableInvalidState => {
                "both operations returned with Q>0, C=false, D=false, and the task remains eligible on recovery CPU".to_string()
            }
            PolicyEventKind::RecoveryDrainEnabled => {
                "bounded recovery enabled D for the orphan LLC queue".to_string()
            }
        }
    }

    fn emit_snapshot_event(
        &self,
        event: &str,
        pid: Option<i32>,
        cpu: Option<u16>,
        selected_target_llc: Option<u16>,
        snapshot: PolicySnapshot,
        note: &str,
    ) {
        emit_static_event(
            self.opts.mode,
            event,
            &self.plan,
            pid,
            cpu,
            selected_target_llc,
            snapshot.mask_generation,
            snapshot.q,
            snapshot.c,
            snapshot.d,
            note,
        );
    }

    fn emit_summary(&mut self, event: &str) {
        let elapsed_ms = self.started_at.elapsed().as_millis();
        let nr_queued = *self.bpf.nr_queued_mut();
        let nr_scheduled = *self.bpf.nr_scheduled_mut();
        let nr_running = *self.bpf.nr_running_mut();
        let nr_user_dispatches = *self.bpf.nr_user_dispatches_mut();
        let nr_kernel_dispatches = *self.bpf.nr_kernel_dispatches_mut();
        let nr_cancel_dispatches = *self.bpf.nr_cancel_dispatches_mut();
        let nr_bounce_dispatches = *self.bpf.nr_bounce_dispatches_mut();
        let nr_failed_dispatches = *self.bpf.nr_failed_dispatches_mut();
        let nr_sched_congested = *self.bpf.nr_sched_congested_mut();
        let note = format!(
            "elapsed_ms={} selected_once={} recovered={} nr_queued={} nr_scheduled={} nr_running={} nr_user_dispatches={} nr_kernel_dispatches={} nr_cancel_dispatches={} nr_bounce_dispatches={} nr_failed_dispatches={} nr_sched_congested={}",
            elapsed_ms,
            self.policy.selected_once(),
            self.policy.recovered(),
            nr_queued,
            nr_scheduled,
            nr_running,
            nr_user_dispatches,
            nr_kernel_dispatches,
            nr_cancel_dispatches,
            nr_bounce_dispatches,
            nr_failed_dispatches,
            nr_sched_congested,
        );
        self.emit_snapshot_event(
            event,
            None,
            Some(self.plan.control_cpu),
            None,
            self.policy.snapshot(),
            &note,
        );
    }
}

fn policy_mode(mode: RunMode) -> PolicyMode {
    match mode {
        RunMode::Report => PolicyMode::Report,
        RunMode::Deterministic => PolicyMode::Deterministic,
        RunMode::Stochastic => PolicyMode::Stochastic,
    }
}

fn policy_plan(plan: &TopologyPlan) -> PolicyPlan {
    PolicyPlan {
        target_cpu: plan.target_cpu,
        target_llc: plan.target_llc,
        recovery_cpu: plan.recovery_cpu,
        recovery_llc: plan.recovery_llc,
        control_cpu: plan.control_cpu,
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_static_event(
    mode: RunMode,
    event: &str,
    plan: &TopologyPlan,
    pid: Option<i32>,
    cpu: Option<u16>,
    selected_target_llc: Option<u16>,
    mask_generation: u64,
    q: usize,
    c: bool,
    d: bool,
    note: &str,
) {
    println!(
        "{{\"source\":\"rustland_adapter\",\"adapter_observed\":true,\"mode\":\"{}\",\"event\":\"{}\",\"pid\":{},\"cpu\":{},\"partition\":0,\"llc\":{},\"selected_target_llc\":{},\"mask_generation\":{},\"q\":{},\"c\":{},\"d\":{},\"pending_enqueues\":0,\"recovery_cpu\":{},\"note\":\"{}\"}}",
        adapter_mode(mode),
        escape_json(event),
        opt_i32(pid),
        opt_u16(cpu),
        plan.target_llc,
        opt_u16(selected_target_llc),
        mask_generation,
        q,
        c,
        d,
        plan.recovery_cpu,
        escape_json(note)
    );
}

fn adapter_mode(mode: RunMode) -> &'static str {
    match mode {
        RunMode::Report => "report_adapter",
        RunMode::Deterministic => "deterministic_adapter",
        RunMode::Stochastic => "stochastic_adapter",
    }
}

fn opt_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
}

fn opt_u16(value: Option<u16>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::new();
    for c in value.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped
}
