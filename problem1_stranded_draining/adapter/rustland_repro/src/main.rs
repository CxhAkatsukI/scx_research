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

use std::collections::VecDeque;
use std::mem::MaybeUninit;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use bpf::{BpfScheduler, DispatchedTask, QueuedTask, RL_CPU_ANY};
use libbpf_rs::OpenObject;
use problem1_stranded_draining::harness::RunMode;
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
    target_q: VecDeque<QueuedTask>,
    published_target_llc: bool,
    drain_target_llc: bool,
    mask_generation: u64,
    selected_once: bool,
    invalid_since: Option<Instant>,
    recovered: bool,
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
            plan,
            target_q: VecDeque::new(),
            published_target_llc: true,
            drain_target_llc: false,
            mask_generation: 0,
            selected_once: false,
            invalid_since: None,
            recovered: false,
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
                            self.emit_event(
                                "dequeue_error",
                                None,
                                Some(self.plan.control_cpu),
                                None,
                                &note,
                            );
                            reported_dequeue_error = true;
                        }
                        break;
                    }
                }
            }

            self.dispatch_ready_tasks()?;
            self.bpf.notify_complete(self.target_q.len() as u64);

            if self.recovered {
                break;
            }
        }

        let exited = self.bpf.exited();
        let event = if self.recovered {
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
        if !self.is_problem_workload(&task) {
            self.dispatch_any(task)?;
            return Ok(());
        }

        if !self.selected_once {
            self.selected_once = true;
            let note = format!("matched workload pid={} comm={}", task.pid, task.comm_str());
            self.emit_event(
                "workload_matched",
                Some(task.pid),
                Some(self.plan.target_cpu),
                Some(self.plan.target_llc),
                &note,
            );
            self.emit_event(
                "enqueue_select",
                Some(task.pid),
                Some(self.plan.target_cpu),
                Some(self.plan.target_llc),
                "enqueue observed the old mask and selected the target LLC",
            );

            if self.opts.mode == RunMode::Deterministic {
                thread::sleep(Duration::from_millis(self.opts.gate_hold_ms));
            }

            self.publish_mask_without_target_llc();
            self.update_observe_queue();
            self.enqueue_commit_to_target_llc(task);
        } else if self.published_target_llc {
            self.enqueue_commit_to_target_llc(task);
        } else {
            self.dispatch_to_recovery(task)?;
        }

        Ok(())
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

    fn publish_mask_without_target_llc(&mut self) {
        self.published_target_llc = false;
        self.mask_generation += 1;
        self.emit_event(
            "publish_mask",
            None,
            Some(self.plan.control_cpu),
            None,
            "partition mask now excludes every CPU in the target LLC",
        );
    }

    fn update_observe_queue(&mut self) {
        if !self.published_target_llc && !self.target_q.is_empty() {
            self.drain_target_llc = true;
        }
        self.emit_event(
            "update_observe_queue",
            None,
            Some(self.plan.control_cpu),
            None,
            "updater enables D only when it observes Q>0",
        );
    }

    fn enqueue_commit_to_target_llc(&mut self, task: QueuedTask) {
        let pid = task.pid;
        self.target_q.push_back(task);
        self.emit_event(
            "enqueue_commit",
            Some(pid),
            Some(self.plan.target_cpu),
            Some(self.plan.target_llc),
            "enqueue committed the LLC selected before the mask update",
        );
    }

    fn dispatch_ready_tasks(&mut self) -> Result<()> {
        if self.target_q.is_empty() {
            return Ok(());
        }

        if self.published_target_llc {
            if let Some(task) = self.target_q.pop_front() {
                self.dispatch_to_target(task)?;
            }
            return Ok(());
        }

        if self.drain_target_llc {
            if let Some(task) = self.target_q.pop_front() {
                self.dispatch_to_recovery(task)?;
                self.recovered = true;
            }
            return Ok(());
        }

        if self.invalid_since.is_none() {
            self.invalid_since = Some(Instant::now());
            self.emit_event(
                "stable_invalid_state",
                self.target_q.front().map(|task| task.pid),
                None,
                None,
                "both operations returned with Q>0, C=false, D=false, and the task remains eligible on recovery CPU",
            );
        }

        if self.invalid_since.is_some_and(|since| {
            since.elapsed() >= Duration::from_millis(self.opts.recovery_delay_ms)
        }) {
            self.drain_target_llc = true;
            self.emit_event(
                "recovery_drain_enabled",
                self.target_q.front().map(|task| task.pid),
                Some(self.plan.recovery_cpu),
                None,
                "bounded recovery enabled D for the orphan LLC queue",
            );
        }

        Ok(())
    }

    fn dispatch_any(&mut self, task: QueuedTask) -> Result<()> {
        let mut dispatched = DispatchedTask::new(&task);
        dispatched.cpu = RL_CPU_ANY;
        dispatched.slice_ns = SLICE_NS;
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }

    fn dispatch_to_target(&mut self, task: QueuedTask) -> Result<()> {
        let mut dispatched = DispatchedTask::new(&task);
        dispatched.cpu = self.plan.target_cpu as i32;
        dispatched.slice_ns = SLICE_NS;
        self.emit_event(
            "dispatch_target",
            Some(task.pid),
            Some(self.plan.target_cpu),
            Some(self.plan.target_llc),
            "dispatching on the target CPU while the target LLC is still published",
        );
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }

    fn dispatch_to_recovery(&mut self, task: QueuedTask) -> Result<()> {
        let mut dispatched = DispatchedTask::new(&task);
        dispatched.cpu = self.plan.recovery_cpu as i32;
        dispatched.slice_ns = SLICE_NS;
        self.emit_event(
            "dispatch_recovery",
            Some(task.pid),
            Some(self.plan.recovery_cpu),
            Some(self.plan.recovery_llc),
            "dispatching through the recovery CPU after D becomes true",
        );
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }

    fn emit_event(
        &self,
        event: &str,
        pid: Option<i32>,
        cpu: Option<u16>,
        selected_target_llc: Option<u16>,
        note: &str,
    ) {
        emit_static_event(
            self.opts.mode,
            event,
            &self.plan,
            pid,
            cpu,
            selected_target_llc,
            self.mask_generation,
            self.target_q.len(),
            self.published_target_llc,
            self.drain_target_llc,
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
            self.selected_once,
            self.recovered,
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
        self.emit_event(event, None, Some(self.plan.control_cpu), None, &note);
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
