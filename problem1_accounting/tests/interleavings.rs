// SPDX-License-Identifier: GPL-2.0-only
//! A finite, task-identified kernel adapter. KernelCommit is a separate step.
use problem1_accounting::word;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum Phase {
    Idle,
    Registered,
    Requested,
    Pending,
    Queued,
    Quiescing,
    Local,
    Removed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct Task {
    phase: Phase,
    llc: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct State {
    word: u64,
    tasks: [Task; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Protocol {
    Fixed,
    QueueOnly,
    ReleaseOnReturn,
    ClearOnFailedConsume,
    OverwriteOtherBit,
}

impl State {
    fn initial() -> Self {
        Self {
            word: 3,
            tasks: [Task {
                phase: Phase::Idle,
                llc: 0,
            }; 2],
        }
    }

    fn queued(&self, llc: u8) -> u64 {
        self.tasks
            .iter()
            .filter(|t| t.llc == llc && matches!(t.phase, Phase::Queued | Phase::Quiescing))
            .count() as u64
    }

    fn obligations(&self, llc: u8) -> u64 {
        self.tasks
            .iter()
            .filter(|t| t.llc == llc && t.phase != Phase::Idle)
            .count() as u64
    }

    fn stranded(&self) -> bool {
        (0..2).any(|l| self.queued(l) > 0 && self.word & (1 << l) == 0 && self.word & (4 << l) == 0)
    }

    fn check_connection(&self) {
        assert_eq!(self.word >> 36, 0);
        for l in 0..2 {
            assert_eq!(word::count(self.word, l), self.obligations(l), "{self:?}");
            assert!(self.queued(l) <= word::count(self.word, l));
            assert!(
                word::count(self.word, l) == 0
                    || self.word & (1 << l) != 0
                    || self.word & (4 << l) != 0,
                "{self:?}"
            );
        }
        assert!(!self.stranded());
    }

    fn successors(self, protocol: Protocol) -> Vec<(String, Self)> {
        let mut out = Vec::new();
        for coverage in 0..4u8 {
            let mut s = self;
            s.word = if protocol == Protocol::QueueOnly {
                (s.word & !3)
                    | coverage as u64
                    | if s.queued(0) > 0 && coverage & 1 == 0 {
                        4
                    } else {
                        0
                    }
                    | if s.queued(1) > 0 && coverage & 2 == 0 {
                        8
                    } else {
                        0
                    }
            } else {
                word::publish(s.word, coverage)
            };
            out.push((format!("config({coverage:02b})"), s));
        }
        for llc in 0..2 {
            let mut s = self;
            s.word = if protocol == Protocol::ClearOnFailedConsume {
                s.word & !(4 << llc)
            } else {
                word::retire(s.word, llc)
            };
            out.push((format!("failed_consume_and_retire({llc})"), s));
        }
        for id in 0..2 {
            let task = self.tasks[id];
            let targets: &[u8] = if task.phase == Phase::Idle {
                &[0, 1]
            } else {
                &[task.llc]
            };
            for &llc in targets {
                let mut s = self;
                let (phase, name) = match task.phase {
                    Phase::Idle => {
                        s.word = word::register(s.word, llc);
                        if protocol == Protocol::OverwriteOtherBit && s.word & (1 << llc) == 0 {
                            s.word = (s.word & !12) | (4 << llc);
                        }
                        (Phase::Registered, "register")
                    }
                    Phase::Registered => (Phase::Requested, "request_insert"),
                    Phase::Requested => {
                        if protocol == Protocol::ReleaseOnReturn {
                            s.word = word::release(s.word, llc);
                        }
                        (Phase::Pending, "callback_return")
                    }
                    Phase::Pending => (Phase::Queued, "kernel_commit"),
                    Phase::Queued => (Phase::Quiescing, "quiescent_before_unlink"),
                    Phase::Quiescing => (Phase::Removed, "kernel_unlink"),
                    Phase::Local => {
                        if word::count(s.word, llc) == 0 {
                            continue;
                        }
                        s.word = word::release(s.word, llc);
                        (Phase::Idle, "running_release")
                    }
                    Phase::Removed => {
                        if word::count(s.word, llc) == 0 {
                            continue;
                        }
                        s.word = word::release(s.word, llc);
                        (Phase::Idle, "safe_release")
                    }
                };
                s.tasks[id] = Task {
                    phase,
                    llc: if phase == Phase::Idle { 0 } else { llc },
                };
                out.push((format!("task{id}.{name}({llc})"), s));
            }
            if task.phase == Phase::Queued {
                let mut s = self;
                s.tasks[id].phase = Phase::Local;
                out.push((
                    format!("task{id}.consume_success_move_to_local({})", task.llc),
                    s,
                ));
            }
            if task.phase == Phase::Pending {
                // Abstract cancellation certificate, not quiescent: the
                // kernel must guarantee this generation will never commit.
                let mut s = self;
                s.tasks[id].phase = Phase::Removed;
                out.push((
                    format!("task{id}.kernel_cancel_no_late_commit({})", task.llc),
                    s,
                ));
            }
        }
        out
    }
}

fn explore(protocol: Protocol, check: bool) -> (usize, usize, Option<Vec<String>>) {
    let start = State::initial();
    let mut todo = VecDeque::from([start]);
    let mut parents: HashMap<State, Option<(State, String)>> = HashMap::from([(start, None)]);
    let mut edges = 0;
    while let Some(s) = todo.pop_front() {
        if check {
            s.check_connection();
        }
        if s.stranded() {
            let mut trace = vec![format!("STRANDED {s:?}")];
            let mut at = s;
            while let Some((prev, action)) = parents[&at].clone() {
                trace.push(action);
                at = prev;
            }
            trace.reverse();
            return (parents.len(), edges, Some(trace));
        }
        for (action, next) in s.successors(protocol) {
            edges += 1;
            if let std::collections::hash_map::Entry::Vacant(e) = parents.entry(next) {
                e.insert(Some((s, action)));
                todo.push_back(next);
            }
        }
    }
    (parents.len(), edges, None)
}

#[test]
fn every_reachable_task_interleaving_preserves_connection() {
    let (states, edges, bad) = explore(Protocol::Fixed, true);
    assert!(bad.is_none());
    assert!(states > 1000);
    println!("fixed: {states} reachable states, {edges} transitions; all connected and safe");
}

#[test]
fn plausible_broken_protocols_have_kernel_commit_counterexamples() {
    for protocol in [
        Protocol::QueueOnly,
        Protocol::ReleaseOnReturn,
        Protocol::ClearOnFailedConsume,
        Protocol::OverwriteOtherBit,
    ] {
        let (_, _, trace) = explore(protocol, false);
        let trace = trace.unwrap_or_else(|| panic!("no counterexample for {protocol:?}"));
        println!("\n{protocol:?}:\n{}", trace.join("\n"));
        assert!(trace.iter().any(|step| step.contains("kernel_commit")));
    }
}
