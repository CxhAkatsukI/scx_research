# Handoff For Claude: Problem 1 Stranded Draining Repro

Last updated: 2026-07-30

This document hands off the next phase of the scheduler-verification playground
for Problem 1. It is intentionally explicit because the next agent must not
skip the real kernel evidence step or accidentally turn the reproducer into a
pure model artifact.

## Repository State

- Primary repo: `~/Documents/scx_research`
- Remote: `git@github.com:CxhAkatsukI/scx_research.git`
- Active branch: `codex/problem1-stranded-draining-repro`
- Latest known baseline before local harness work: `f33b8f4 docs(problem1): add Claude handoff`
- Working tree at this document's original handoff point: clean before the
  handoff document was added
- Out of scope: `scx_simple`
- Current folder for this task: `problem1_stranded_draining/`

The branch already tracks `origin/codex/problem1-stranded-draining-repro`.
Whenever you make a commit, push it immediately to that remote branch.

2026-07-30 update: the next proposal implementation is on
`codex/verus-policy-core-split`. The real rustland reproduction is being split
into three layers:

```text
main.bpf.c / scx_rustland_core  -> fixed sched-ext adapter
rustland_repro/src/main.rs      -> runtime wrapper / action interpreter
src/policy_core.rs              -> Rust embedded policy DSL / Verus-facing core
```

The old statement that the project lacks kernel evidence is stale. The
deterministic VM trace in `traces/vm_rustland_deterministic_20260724_224952/`
showed the workload entering SCHED_EXT, the adapter observing the enqueue path,
the invalid `Q>0, C=false, D=false` state, bounded recovery, and clean unload.
After the split, the acceptance test is to reproduce the same trace shape again
from the new three-layer implementation.

## User Goal

Build a playground for the first scheduler-verification case study: a task can
be stranded in an LLC-local queue after the partition mask no longer contains
any CPU in that LLC, while the drain bit remains disabled.

The goal is not to prove the scheduler yet. The immediate goal is to write a
simple, convincing Rust/sched-ext experiment that demonstrates the buggy
behavior with code, comments, traces, and eventually kernel evidence.

The user will run dangerous or privileged tests on the CachyOS VM manually.
Develop locally first, commit and push, and let the user test from the remote
machine. Do not run risky remote VM commands yourself.

## Problem Statement

For partition `i` and LLC `l`, track three facts:

- `Q(i, l)`: at least one runnable task is queued in the partition's LLC-local
  queue for LLC `l`.
- `C(i, l)`: the currently published partition CPU mask contains at least one
  CPU in LLC `l`.
- `D(i, l)`: the drain bit for that partition/LLC queue is enabled.

The invalid stable state is:

```text
Q(i, l) && !C(i, l) && !D(i, l)
```

This is not a torn mask-publication bug. It is a missing handoff between enqueue
and mask update:

1. `enqueue` observes the old mask and selects LLC0.
2. The mask updater publishes a new mask that removes all CPUs from LLC0.
3. The updater observes the LLC0 queue as empty, so it leaves `D=false`.
4. `enqueue` commits its stale LLC0 placement after the updater returns.

Either serial order is safe:

- If enqueue commits first, the updater sees `Q>0` and enables draining.
- If the mask update publishes first, enqueue should avoid the orphan LLC.

The failure exists in the concurrent window between those serial orders.

## Current Implementation

The first commit provided only the protocol model. Later work added a dry-run
harness, topology planning, expanded trace fields, workload/preflight utilities,
local CLIs, and a VM-only rustland adapter scaffold. The 2026-07-24 VM run
provided real deterministic kernel evidence; the 2026-07-30 split keeps that
real adapter path but moves its policy semantics into `src/policy_core.rs`.

Implemented files:

- `src/protocol.rs`: abstract Rust protocol state and transition model.
- `src/policy_core.rs`: executable Rust embedded policy DSL / Verus-facing core
  shared by the real adapter wrapper.
- `src/trace.rs`: JSON trace event structure and model trace generator.
- `src/bin/protocol_trace.rs`: CLI for printing or writing the model trace.
- `src/topology.rs`: sysfs/synthetic topology planning for target, recovery,
  and control CPUs.
- `src/harness.rs`: local dry-run report, deterministic, and stochastic modes.
- `src/linux.rs`: small Linux helpers for affinity, SCHED_EXT opt-in, uid, and
  PATH checks.
- `src/bin/problem1_harness.rs`: local dry-run harness CLI.
- `src/bin/problem1_workload.rs`: CPU-bound workload with progress-file and
  stop-file.
- `src/bin/problem1_vm_preflight.rs`: non-loading VM environment preflight.
  `--export-env` prints the detected CPU plan as shell variables.
- `adapter/rustland_repro/`: VM-only `scx_rustland_core` scheduler scaffold.
  It enables partial switching, drains kernel events, calls `PolicyCore`, and
  interprets policy actions as real dispatches, bounded gates, and stdout JSONL
  event logging.
- `harness/run_rustland_vm.sh`: VM-only wrapper that runs preflight, builds the
  adapter/workload, captures adapter JSONL and workload progress, and performs
  bounded cleanup.
- `traces/protocol_model_deterministic.json`: canonical model-only trace.
- `traces/dry_run_deterministic.json`: canonical dry-run harness trace.
- `adapter/README.md`: adapter boundary notes.
- `harness/README.md`: harness boundary notes.
- `README.md`: overview, invariant, safety rules, and local checks.

Current local checks:

```sh
cd ~/Documents/scx_research/problem1_stranded_draining
cargo test
cargo run --bin protocol_trace -- --write traces/protocol_model_deterministic.json
cargo run --bin problem1_harness -- deterministic --synthetic-topology
cargo run --bin problem1_harness -- report --synthetic-topology
cargo run --bin problem1_harness -- stochastic --attempts 64 --synthetic-topology
cargo run --bin problem1_workload -- --progress-file /tmp/problem1.progress --stop-file /tmp/problem1.stop --max-iters 1000000
cargo run --bin problem1_vm_preflight
cargo run --bin problem1_vm_preflight -- --export-env
cargo build --manifest-path adapter/rustland_repro/Cargo.toml
sudo harness/run_rustland_vm.sh deterministic
```

Important: `protocol_model_deterministic.json` has
`adapter_observed=false`. It is not kernel evidence.

Important: dry-run harness traces also have `adapter_observed=false`. They
freeze the run contract and trace shape, but still are not kernel evidence.

## Current VM Evidence

The first real deterministic VM trace has been produced on the CachyOS test
host:

```text
problem1_stranded_draining/traces/vm_rustland_deterministic_20260724_224952/
```

The clean adapter JSONL contains the expected adapter-observed sequence:
`workload_matched`, `enqueue_select`, `publish_mask`, `update_observe_queue`,
`enqueue_commit`, `stable_invalid_state`, `recovery_drain_enabled`,
`dispatch_recovery`, and `adapter_summary_recovered`.

The workload stderr/live files show the workload opted into `SCHED_EXT`
(`policy=7`, `ext.enabled=1`), and the harness ended with
`sched_ext_state_after=disabled`.

## What Is Not Done Yet

The following are still missing and should be implemented next:

- Promote the deterministic VM evidence into a stable documented acceptance
  test instead of relying on one manually reviewed trace directory.
- Add real adapter-backed `report` and `stochastic` modes.
- Cross-check protocol fields against more direct BPF/kernel observations where
  possible, especially pending enqueue state and queue length.
- Add stronger run instructions for the user-facing CachyOS workflow.

## Hard Safety Requirements

Treat these as non-negotiable:

- The scheduler must be loadable, runnable, and unloadable.
- Do not switch the whole system into the custom scheduler.
- Use sched-ext partial switching only.
- SSH, systemd, shell/control tasks, and health checks must stay on the default
  scheduler.
- Never include every vCPU in the experiment.
- Prefer CPU0/LLC0 as the orphaned queue target, CPU1/LLC1 as the eligible
  recovery CPU, and CPU2 as the harness/control CPU. If topology differs, detect
  and explain the chosen mapping.
- Workload affinity should include the orphan target and an eligible recovery
  CPU, for example `{CPU0, CPU1}`.
- The task must remain eligible outside the orphan LLC; otherwise affinity could
  explain the lack of progress.
- No automatic reboot.
- No remote privileged testing unless the user explicitly asks and the platform
  permits it.
- Always implement recovery before attempting real loading.

## Deterministic Gate Semantics

The user clarified the intended gate model:

1. Report mode detects and reports that a vulnerable window exists.
2. Deterministic mode may hold a real vulnerable window for a bounded time to
   make the interleaving reproducible.
3. Stochastic mode does not hold the window; it repeats naturally and reports
   whether the race was hit.

The deterministic gate must not fabricate `Q`, `C`, or `D`. It may only control
the ordering of real operations that could happen under the scheduler protocol.

The stable failure should only be reported after both the enqueue path and mask
update path have returned, with no pending enqueue left:

```text
Q > 0
C = false
D = false
pending_enqueues = 0
task is still eligible on another online CPU
```

## Expected Run Modes

### `report`

Purpose: prove that the vulnerable window exists without artificially holding
it.

Expected output:

- selected topology and CPU roles;
- observed enqueue selected LLC0 using old mask generation;
- observed mask update removed LLC0 CPUs;
- whether the updater observed an empty queue;
- whether the later state became invalid;
- cleanup and health status.

### `deterministic`

Purpose: make the legal interleaving reproducible by briefly holding the real
window.

Expected output:

- all `report` fields;
- gate arm/release timestamps;
- bounded gate duration;
- evidence that the gate did not directly set `Q`, `C`, or `D`;
- stable invalid-state trace;
- recovery trace.

### `stochastic`

Purpose: run many natural attempts without holding the window.

Expected output:

- number of attempts;
- hit count;
- hit rate;
- representative trace for any hit;
- cleanup and health status.

A stochastic hit is useful but should not be required for the first milestone if
deterministic mode produces a valid real trace.

## Evidence Contract

A real trace should include enough fields to compare the abstract model and the
adapter. Suggested fields:

- event sequence number;
- timestamp;
- mode;
- event name;
- CPU and task id;
- partition id;
- LLC id;
- mask generation;
- `Q` from protocol and adapter/BPF;
- `C` from protocol and adapter/BPF;
- `D` from protocol and adapter/BPF;
- pending enqueue count;
- selected target LLC;
- task affinity;
- task progress counter;
- adapter observed flag;
- note.

The trace should explicitly mark whether each observation is model-only or
adapter-observed. Do not overclaim model traces as kernel evidence.

Minimum real evidence:

- `/sys/kernel/sched_ext/state` becomes enabled during the run.
- The workload is switched into SCHED_EXT, but control tasks are not.
- The invalid state is observed after both competing operations return.
- The task is still eligible for another online CPU outside LLC0.
- The task makes no progress while stranded.
- Recovery enables draining or otherwise performs the intended handoff.
- The task makes progress after recovery.
- The scheduler unloads.
- `/sys/kernel/sched_ext/state` is disabled after cleanup.
- SSH/system health survives.

## Implementation Guidance

Keep protocol and adapter separate:

- Protocol: pure Rust state machine, future Verus target, no kernel dependencies.
- Adapter: thin sched-ext/BPF boundary that exposes real events and state.
- Harness: workload, topology detection, gates, trace recording, recovery, health
  checks, and user-facing commands.

The existing protocol API names are intentionally aligned with the race:

- `enqueue_select`
- `publish_mask`
- `update_observe_queue`
- `enqueue_commit`

The adapter should preserve this vocabulary in event names so traces are easy to
cross-check.

There is a local upstream sched-ext checkout that can be used as a reference:

```text
~/Documents/sched-ext/sched-test/scx
```

Do not modify that checkout. It may contain unrelated user changes. Use it only
to inspect current APIs, examples, and build patterns.

Because sched-ext APIs are moving, verify against primary/local sources rather
than relying on memory. Prefer local kernel headers and the checked-out scx
examples when building the real adapter. If internet lookup is needed, use
primary sources such as the official `sched-ext/scx` repository.

## Suggested Milestones

Commit and push after each meaningful milestone.

1. Document the concrete VM test command and build prerequisites.
2. Add the adapter build scaffold behind an explicit feature or VM-only command.
3. Add a loadable partial-switch scheduler that immediately unloads cleanly.
4. Add workload creation, affinity control, progress counter, and health checks.
5. Add adapter-visible `Q`, `C`, `D`, mask generation, and enqueue event logging.
6. Add `report` mode.
7. Add bounded deterministic gate and recovery.
8. Add stochastic repeat mode.
9. Add trace cross-checking and JSON output.
10. Update README with exact user-run commands and evidence interpretation.

Do not wait until the entire feature is done before committing. Each commit
should leave the repository in a coherent state.

## Things To Avoid

- Do not touch `scx_simple`.
- Do not add a fake trace with `adapter_observed=true`.
- Do not treat protocol-only tests as proof that the bug exists in the kernel.
- Do not leave a loaded scheduler running after a failed test.
- Do not write an infinite or unbounded gate.
- Do not directly set `Q`, `C`, or `D` just to create the desired trace.
- Do not erase unrelated local changes in any repo.
- Do not use destructive git commands.

## Suggested Prompt For Claude

Copy the following prompt into Claude:

```text
You are taking over a scheduler-verification playground task in a local Linux
workspace. Please work carefully and commit/push after each meaningful milestone.

Context:
- Main repo: ~/Documents/scx_research
- Branch: codex/problem1-stranded-draining-repro
- Remote: git@github.com:CxhAkatsukI/scx_research.git
- Task folder: problem1_stranded_draining/
- Out of scope: scx_simple
- Current state: the repo has a pure Rust protocol model, a local dry-run
  harness, workload/preflight utilities, and a VM-only rustland adapter scaffold
  for Problem 1, but no VM-tested kernel evidence.

First, read these files and the recent commit history:
- problem1_stranded_draining/HANDOFF_FOR_CLAUDE.md
- problem1_stranded_draining/README.md
- problem1_stranded_draining/adapter/README.md
- problem1_stranded_draining/harness/README.md
- problem1_stranded_draining/src/protocol.rs
- problem1_stranded_draining/src/trace.rs
- git log --oneline -8

The target bug:
For partition i and LLC l, define:
- Q(i,l): runnable task queued in the LLC-local queue
- C(i,l): published partition CPU mask contains at least one CPU in that LLC
- D(i,l): drain bit for that LLC queue is enabled

The invalid stable state is:
Q(i,l) && !C(i,l) && !D(i,l)

The race is:
1. enqueue observes the old mask and selects LLC0;
2. mask update removes all CPUs from LLC0;
3. updater observes the LLC0 queue as empty and leaves D=false;
4. enqueue commits stale placement to LLC0 after the updater returns.

Your job:
Continue implementation locally. Build the missing real playground pieces:
- a loadable/unloadable sched-ext adapter;
- partial switching only for experiment workload tasks;
- real VM-backed workload affinity, progress counter, deterministic gates,
  stochastic stress, recovery, cleanup, and health checks;
- report, deterministic, and stochastic modes;
- JSON traces that cross-check protocol state against adapter/BPF state;
- README instructions that the user can run on a CachyOS VM.

Safety constraints:
- Do not switch the whole system into the custom scheduler.
- Do not include every vCPU in the experiment.
- Keep SSH/system/control tasks on the default scheduler.
- Prefer CPU0/LLC0 as orphan target, CPU1/LLC1 as recovery CPU, CPU2 as control
  CPU; detect topology and explain if a different mapping is needed.
- Workload affinity must keep the task eligible outside the orphan LLC.
- Deterministic gates may only hold a real legal vulnerable window for a bounded
  time. They must not fabricate Q/C/D.
- Implement cleanup and recovery before real loading.
- No automatic reboot.
- Do not run risky remote VM commands yourself; the user will test on CachyOS.
- Do not touch scx_simple.
- Do not overclaim model-only traces as kernel evidence.

Commit discipline:
- Before editing, inspect status.
- Never revert unrelated user changes.
- After each coherent progress commit, immediately push to origin on the same
  branch.

Validation:
- Locally run protocol tests:
  cd ~/Documents/scx_research/problem1_stranded_draining && cargo test
- For kernel adapter work, provide VM-facing commands and make failures explicit
  if the local machine lacks sched_ext support.

Please start by summarizing what is already implemented and then proceed with
the next missing milestone.
```
