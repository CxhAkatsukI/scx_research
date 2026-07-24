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

The current local implementation covers the protocol model, a dry-run harness,
JSON trace generation, and local tests. Kernel loading, BPF state capture,
workload evidence, and real health checks are intentionally left for the adapter
milestones.

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
├── src/          # Rust protocol model, dry-run harness, and trace generator.
└── traces/       # Small canonical traces. Bulk logs stay untracked.
```

The protocol layer is the future Verus target. It exposes the fine-grained
transitions directly:

- `enqueue_select`
- `publish_mask`
- `update_observe_queue`
- `enqueue_commit`

The adapter must remain thin: it can translate sched-ext callbacks and BPF/user
events, but it must not own the protocol semantics.

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
queue/mask/drain logic in Rust. It should be built and run only on a sched-ext
capable VM.

`harness/run_rustland_vm.sh` captures adapter JSONL, workload stderr, and a
workload progress counter under `traces/vm_rustland_*`. It still needs VM
execution before its output can be treated as kernel evidence.

## Safety Rules For The Real Adapter

- Use partial switching only. SSH, systemd, and control tasks must stay on the
  default scheduler.
- Never include every vCPU in the experiment.
- Use CPU0/LLC0 as the orphaned queue target, CPU1/LLC1 as the eligible recovery
  CPU, CPU2 for harness/control activity, and reserve the remaining CPUs.
- `problem1_workload --sched-ext` is only for the VM stage after the adapter is
  ready and partial switching is enabled.
- Deterministic gates may only hold a real legal window for a bounded time. They
  must not directly write `Q`, `C`, or `D`.
- Random stress may report a hit rate, but a random hit is not required for
  stage completion.
- No automatic reboot. Reboot requires explicit human authorization.
