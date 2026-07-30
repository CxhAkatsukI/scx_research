# Adapter Boundary

This directory contains the sched-ext adapter boundary for the Problem 1
playground.

The first real-adapter scaffold lives in `rustland_repro/`. It uses
`scx_rustland_core`: the generic sched-ext BPF backend handles kernel
attachment, while the Problem 1 queue/mask/drain policy is implemented by the
top-level `src/policy_core.rs`. The rustland `main.rs` file is now a runtime
wrapper: it drains BPF ringbuf events, builds `PolicyInput`s, and interprets
`PolicyAction`s as real dispatches, bounded sleeps, and JSONL events. This keeps
the runnable scheduler tied to the same policy core that will become the
Verus-facing input.

The adapter must:

- load only through the controlled harness;
- use partial switching for the experiment workload;
- expose real adapter/BPF state for `Q`, `C`, and `D`;
- forward protocol decisions instead of reimplementing scheduler semantics;
- unload cleanly after every run.

The intended verification split is:

```text
main.bpf.c / scx_rustland_core  -> fixed adapter, specified once
rustland_repro/src/main.rs      -> runtime wrapper and action interpreter
src/policy_core.rs              -> Rust embedded policy DSL / Verus target
```

`src/policy_core.rs` must stay free of BPF calls, sleeps, JSON printing, real
time, and sysfs discovery. It may emit abstract actions such as dispatching a
task or holding a deterministic gate; the wrapper performs those effects.

## VM-Only Build

Do not run this on the local WSL environment. First run the non-loading
preflight:

```sh
cd problem1_stranded_draining
cargo run --bin problem1_vm_preflight
```

On a sched-ext capable VM, build the rustland adapter:

```sh
cd problem1_stranded_draining
cargo build --manifest-path adapter/rustland_repro/Cargo.toml
```

The adapter enables partial switching through `scx_rustland_core` and disables
the rustland backend's built-in idle fast path so opt-in workload wakeups are
delivered to the Rust scheduler instead of being directly dispatched in BPF. The
workload must opt into SCHED_EXT explicitly after the adapter is loaded:

```sh
cargo run --manifest-path adapter/rustland_repro/Cargo.toml -- \
  --mode deterministic \
  --recovery-delay-ms 100

cargo run --bin problem1_workload -- \
  --progress-file /tmp/problem1.progress \
  --stop-file /tmp/problem1.stop \
  --cpu-list 0,1 \
  --sched-ext
```

The scaffold prints JSONL protocol events to stdout with
`adapter_observed=true`; non-event exit diagnostics go to stderr. The next
adapter milestone should add a harness command that captures those JSONL events
beside workload progress samples and cleanup checks.
