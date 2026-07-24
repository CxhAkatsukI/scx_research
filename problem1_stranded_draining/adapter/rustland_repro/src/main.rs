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

const SLICE_NS: u64 = 5_000_000;
const SCHED_NAME: &str = "p1_strand";

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

        println!(
            "topology target=CPU{}/LLC{} recovery=CPU{}/LLC{} control=CPU{}",
            plan.target_cpu,
            plan.target_llc,
            plan.recovery_cpu,
            plan.recovery_llc,
            plan.control_cpu
        );
        println!("loading rustland backend with partial switching enabled");

        let open_opts = LibbpfOpts::default().into_bpf_open_opts();
        let bpf = BpfScheduler::init(
            open_object,
            open_opts,
            0,
            true,
            opts.debug,
            true,
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
        while !self.bpf.exited() && self.started_at.elapsed() < self.max_runtime() {
            while let Ok(Some(task)) = self.bpf.dequeue_task() {
                self.handle_queued_task(task)?;
            }

            self.dispatch_ready_tasks()?;
            self.bpf.notify_complete(self.target_q.len() as u64);

            if self.recovered {
                break;
            }
        }

        let exit = self.bpf.shutdown_and_report()?;
        println!("scheduler exit: {:?}", exit);
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
            println!(
                "enqueue_select pid={} old_mask_generation={} target_llc={} q={} c={} d={}",
                task.pid,
                self.mask_generation,
                self.plan.target_llc,
                self.target_q.len(),
                self.published_target_llc,
                self.drain_target_llc
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

        task.comm_str().contains("problem1_workload")
    }

    fn publish_mask_without_target_llc(&mut self) {
        self.published_target_llc = false;
        self.mask_generation += 1;
        println!(
            "publish_mask generation={} target_llc={} c=false",
            self.mask_generation, self.plan.target_llc
        );
    }

    fn update_observe_queue(&mut self) {
        if !self.published_target_llc && !self.target_q.is_empty() {
            self.drain_target_llc = true;
        }
        println!(
            "update_observe_queue q={} c={} d={}",
            self.target_q.len(),
            self.published_target_llc,
            self.drain_target_llc
        );
    }

    fn enqueue_commit_to_target_llc(&mut self, task: QueuedTask) {
        let pid = task.pid;
        self.target_q.push_back(task);
        println!(
            "enqueue_commit pid={} target_llc={} q={} c={} d={}",
            pid,
            self.plan.target_llc,
            self.target_q.len(),
            self.published_target_llc,
            self.drain_target_llc
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
            println!(
                "stable_invalid_state q={} c=false d=false pending_enqueue=0 eligible_recovery_cpu={}",
                self.target_q.len(),
                self.plan.recovery_cpu
            );
        }

        if self.invalid_since.is_some_and(|since| {
            since.elapsed() >= Duration::from_millis(self.opts.recovery_delay_ms)
        }) {
            self.drain_target_llc = true;
            println!(
                "recovery_drain_enabled target_llc={} q={}",
                self.plan.target_llc,
                self.target_q.len()
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
        println!("dispatch_target pid={} cpu={}", task.pid, dispatched.cpu);
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }

    fn dispatch_to_recovery(&mut self, task: QueuedTask) -> Result<()> {
        let mut dispatched = DispatchedTask::new(&task);
        dispatched.cpu = self.plan.recovery_cpu as i32;
        dispatched.slice_ns = SLICE_NS;
        println!("dispatch_recovery pid={} cpu={}", task.pid, dispatched.cpu);
        self.bpf.dispatch_task(&dispatched)?;
        Ok(())
    }
}
