# Problem 1: Stranded CPU Draining

This playground isolates the first scheduler-verification case study: a task can
be queued on an LLC-local queue after the partition mask has stopped containing
any CPU in that LLC, while the drain bit for that queue is still disabled.

The playground is intentionally self-contained. It is not `scx_simple`, and it
does not reuse the old simulator. The first stage is a Rust protocol model that
captures the interleaving we want to exercise before we attach it to a real
sched-ext adapter. The second local stage adds a dry-run harness for topology
planning, run modes, recovery events, and trace shape; it still does not load a
kernel scheduler.

## Stage 1 Goal

Produce a real, reproducible Problem 1 trace with these hard facts:

1. The scheduler is loaded only by the controlled harness, never as a persistent
   system service.
2. Only the experiment workload is switched into SCHED_EXT.
3. After both the mask update and the concurrent enqueue return, the observed
   state is stable: `Q > 0`, `C = false`, and `D = false`.
4. The stranded task is still eligible for an online CPU outside the orphan LLC,
   so affinity is not the explanation.
5. The harness recovers the task, unloads the scheduler, verifies sched-ext is
   disabled, and verifies SSH/system health.

The current implementation covers the protocol model, a dry-run harness, JSON
trace generation, local tests, and a VM-only rustland adapter that loads through
the controlled harness. The latest milestone splits the real adapter path into a
fixed BPF adapter, a runtime wrapper, and a Verus-friendly policy core.

## Invariant

For each partition `i` and LLC `l`:

- `Q(i, l)`: there is at least one runnable task queued for partition `i` in
  LLC-local queue `l`.
- `C(i, l)`: the currently published partition CPU mask contains at least one
  CPU in LLC `l`.
- `D(i, l)`: the drain bit for partition `i`, LLC `l` is enabled.

The invalid stable state is:

```text
Q(i, l) && !C(i, l) && !D(i, l)
```

The race is not a torn mask publication. The bug is a missing hand-off across
three operations:

1. enqueue observes the old mask and selects an LLC-local queue;
2. mask update publishes a new mask and observes the queue as empty;
3. enqueue commits using its stale selection.

Either serial order is safe: if enqueue commits first, the updater sees `Q`; if
the update publishes first, enqueue should avoid the orphan LLC. The bad state
appears in the gap between those two serial orders.

## Layout

```text
problem1_stranded_draining/
├── adapter/      # Future sched-ext/BPF integration boundary.
├── harness/      # Harness boundary notes and future VM-facing scripts.
├── src/          # Policy core, protocol model, dry-run harness, and traces.
├── VERUS_POLICY_CORE.md
└── traces/       # Small canonical traces. Bulk logs stay untracked.
```

The current implementation separates the real reproduction into three layers:

1. Fixed sched-ext adapter: `adapter/rustland_repro/main.bpf.c` and the
   generated `scx_rustland_core` wrapper own kernel hooks, ring buffers, DSQs,
   CPU kicks, and partial switching.
2. Runtime wrapper: `adapter/rustland_repro/src/main.rs` loads BPF, drains
   queued tasks, recognizes the experiment workload, applies bounded real-time
   gates, prints JSONL, and translates policy actions into `dispatch_task`
   calls.
3. Policy core: `src/policy_core.rs` is the executable policy semantics. It is a
   Rust embedded DSL/IR: `PolicyInput + PolicyState -> PolicyAction`. It owns
   `Q`, `C`, `D`, mask generation, first-enqueue selection, invalid-state
   reporting, and recovery-drain state. It does not call BPF, sleep, print, read
   the clock, or inspect sysfs.

The older protocol model remains as an explanation and oracle for the race. It
exposes the fine-grained abstract transitions directly:

- `enqueue_select`
- `publish_mask`
- `update_observe_queue`
- `enqueue_commit`

The adapter must remain thin: it can translate sched-ext callbacks and BPF/user
events, but it must not reimplement the policy semantics already owned by
`PolicyCore`.

## Verus Direction

The verification boundary is deliberately narrower than arbitrary sched-ext
source code. We assume schedulers are written as userspace policies on top of a
fixed rustland-style adapter. The adapter gets a reusable environment
specification; each scheduler exposes a Verus-friendly policy core.

For this milestone the conversion plan is documented in
`VERUS_POLICY_CORE.md` and summarized here:

1. Keep `src/policy_core.rs` in a restricted Rust subset: explicit state,
   explicit input enums, explicit action enums, no real IO, no wall-clock time,
   no BPF calls, and no hidden side effects.
2. Feed the same core to the real runtime wrapper and to a Verus-facing wrapper.
   The runtime interprets `PolicyAction::Dispatch` as BPF user-ringbuf work,
   while Verus treats it as an abstract action.
3. Specify the fixed adapter once: ringbuf enqueue delivery, dispatch action
   delivery, DSQ insertion, and CPU kick are environment assumptions rather than
   per-policy code.
4. Leave a full automatic Rust-to-Verus converter as a later goal. The current
   claim is not "verify arbitrary existing sched-ext implementations"; it is
   "schedulers written against this policy interface are executable and
   analyzable."

## Local Checks

Run the protocol tests without root privileges:

```sh
cd problem1_stranded_draining
cargo test
```

Generate the model-only trace:

```sh
cargo run --bin protocol_trace -- --write traces/protocol_model_deterministic.json
```

Run the local dry-run harness:

```sh
cargo run --bin problem1_harness -- deterministic --synthetic-topology
cargo run --bin problem1_harness -- report --synthetic-topology
cargo run --bin problem1_harness -- stochastic --attempts 64 --synthetic-topology
```

Run the CPU-bound workload locally without sched-ext:

```sh
cargo run --bin problem1_workload -- \
  --progress-file /tmp/problem1.progress \
  --stop-file /tmp/problem1.stop \
  --max-iters 1000000
```

Check a VM before any scheduler loading:

```sh
cargo run --bin problem1_vm_preflight
```

Build the VM-only rustland adapter scaffold:

```sh
cargo build --manifest-path adapter/rustland_repro/Cargo.toml
```

Run the VM-only rustland evidence harness:

```sh
sudo harness/run_rustland_vm.sh deterministic
```

The checked-in `protocol_model_deterministic.json` is not kernel evidence. It is
a compact trace of the abstract interleaving that the real adapter must later
reproduce and cross-check against BPF/adapter state.

The dry-run harness also emits `adapter_observed=false`. It is useful for
freezing the run contract, trace fields, topology role names, and recovery
events before attaching the real sched-ext adapter.

The `adapter/rustland_repro` crate is the first real-adapter scaffold. It uses
`scx_rustland_core` with partial switching enabled and keeps the Problem 1
queue/mask/drain logic in Rust. Its test slice is intentionally tiny so the
opt-in workload reaches the userspace enqueue path quickly. Its build script
also disables rustland's queued-wakeup optimization so wakeups are not hidden
behind the kernel's local wakeup fast path. It should be built and run only on a
sched-ext capable VM.

`harness/run_rustland_vm.sh` captures adapter JSONL, workload stderr, live
`ps`/`chrt`/`proc` state, adapter and workload PIDs, and a workload progress
counter under `traces/vm_rustland_*`. The adapter emits a summary event on exit
or timeout so failed VM runs still show whether BPF reported queued or
direct-dispatched tasks.

Latest VM evidence: `traces/vm_rustland_deterministic_20260724_224952/` on the
CachyOS test host produced a real adapter-observed deterministic recovery trace:
`workload_matched`, `enqueue_select`, `publish_mask`, `update_observe_queue`,
`enqueue_commit`, `stable_invalid_state`, `recovery_drain_enabled`,
`dispatch_recovery`, and `adapter_summary_recovered`. The workload was running
as `SCHED_EXT` with `ext.enabled=1`, and the harness reported
`sched_ext_state_after=disabled` after cleanup.

## Safety Rules For The Real Adapter

- Use partial switching only. SSH, systemd, and control tasks must stay on the
  default scheduler.
- Never include every vCPU in the experiment.
- Use CPU0/LLC0 as the orphaned queue target, CPU1/LLC1 as the eligible recovery
  CPU, CPU2 for harness/control activity, and reserve the remaining CPUs.
- `problem1_workload --sched-ext` is only for the VM stage after the adapter is
  ready and partial switching is enabled.
- The VM harness gates the workload on a FIFO after opt-in and releases it from
  the control task, creating a real external wakeup before the bounded sleep and
  `sched_yield()` probes.
- Deterministic gates may only hold a real legal window for a bounded time. They
  must not directly write `Q`, `C`, or `D`.
- Random stress may report a hit rate, but a random hit is not required for
  stage completion.
- No automatic reboot. Reboot requires explicit human authorization.
