# Harness Boundary

This directory will contain workload drivers, mask-update orchestration,
deterministic gates, stochastic stress, recovery, and health checks.

The Rust crate now has a local dry-run harness in `src/harness.rs` and a CLI in
`src/bin/problem1_harness.rs`. That code freezes the run modes and JSON trace
shape without loading sched-ext:

```sh
cd problem1_stranded_draining
cargo run --bin problem1_harness -- deterministic --synthetic-topology
cargo run --bin problem1_harness -- report --synthetic-topology
cargo run --bin problem1_harness -- stochastic --attempts 64 --synthetic-topology
```

All dry-run events keep `adapter_observed=false`.

The crate also includes two VM-prep binaries:

```sh
cargo run --bin problem1_workload -- \
  --progress-file /tmp/problem1.progress \
  --stop-file /tmp/problem1.stop \
  --max-iters 1000000

cargo run --bin problem1_vm_preflight
```

`problem1_workload --sched-ext` is intentionally opt-in and should only be used
after the real adapter enables partial switching.

The first VM-side wrapper is:

```sh
sudo harness/run_rustland_vm.sh deterministic
```

It runs preflight, exports the detected CPU plan, builds the workload and
rustland adapter, runs the built binaries directly, captures adapter JSONL,
samples workload progress and live process state, and performs bounded cleanup
through a stop file and `SIGINT` to the adapter. It is not run locally.

The first real-load milestone must prove that:

- `/sys/kernel/sched_ext/state` becomes enabled during the experiment;
- the invalid state is observed only after the enqueue and mask update both
  return;
- the workload can be recovered onto an eligible CPU;
- sched-ext is disabled after cleanup;
- SSH/system health survives the run.

No root or kernel-loading commands are included in the initial protocol commit.
