# Layered: DSQ visibility and drain retirement

An independent case study of the **actual scx_layered enqueue tail and selected
drain iteration**, not the mitosis logical-accounting protocol.

## Result

For the pinned layered statements and Linux v6.16 enqueue semantics, a completed
model execution can leave `Q > 0 && !C && !D`. The enqueue callback requests
insertion and enables draining; another CPU drains the old task and clears the
bit; the new task lands only after enqueue returns.

This is a **source-backed, fragment-level counterexample**, not a formal
verification of full layered. The CachyOS experiments below extend the evidence
to task-level stalls on Linux 6.18.8: default anti-stall eventually rescues the
task, while disabling it leads to the kernel sched_ext watchdog. This is not a
proof of permanent starvation with default anti-stall.

## Run

```sh
make -C drain_protocol_case_study/layered check
```

Uses the existing `lachesis_step1` Verus build and Rust 1.98.0; override
`VERUS_ROOT` and `RUSTC` if needed. Python 3, Git, a C compiler and network access
for uncached source evidence are required. Downloaded sources and artifacts stay
in ignored `dep/` and `build/` directories. No root privileges are needed.

Expected results:

- Verus: **32 verified, 0 errors**.
- Hypothetical immediate insertion: **225 schedules, 0 bad**.
- v6.16 deferred insertion: **210 schedules, 45 bad**.
- Every explored C/Rust schedule has identical operation receipts and final Q/D.
- Four mutations are rejected by proof obligations.

The immediate variant changes the kernel model, **not the scheduler**. It is a
negative control showing how an overstrong API contract hides this race, not a
proposed fix. These counts exhaust only one enqueue + one drain with the fixed
initial state and eligible-consumption host binding, not all scheduler workloads.

## CachyOS VM smoke test

From the checkout in the VM, run:

```sh
bash drain_protocol_case_study/layered/scripts/vm-smoke.sh
```

The script builds upstream `scx_layered` v1.1.4, checks that its BPF source
matches the audited blob, and asks for `sudo` only to run a bounded 15-second
load with the upstream example configuration. It refuses to run outside a VM or
attach over an active scheduler. The log path is printed at startup; logs and
build artifacts stay in `layered/.vm-run/`. Use `--preflight` to check the
environment without fetching, building, or loading anything.

This checks attach and detach only. It does not exercise or observe the drain
race. The VM kernel is recorded in the log and may differ from the model's v6.16.

## CachyOS drain-race experiment

```sh
bash drain_protocol_case_study/layered/scripts/vm-race.sh
```

The script builds an instrumented copy of the same release. It runs a
`stress-ng` layer with alternating load to exercise LLC shrink and keeps
wakeups active between bursts. It uses `dispatch_enqueue` fentry/fexit and
layered's own drain checks to correlate the same DSQ. The monitor looks for
request+enable, a later clear and empty recheck, and then that task landing
with observed `Q > 0 && !C && !D`. The probes do not change scheduling decisions,
but can affect timing and do not read Q/C/D as an atomic snapshot.

`run.log` reports path coverage and the result; `trace.log` has raw records and
layer stats. A missing match does not rule out the race. Use `--build-only` to
check the build without attaching; `RACE_SECONDS` defaults to 75.

## CachyOS stall experiment

```sh
bash drain_protocol_case_study/layered/scripts/vm-stall.sh
bash drain_protocol_case_study/layered/scripts/vm-stall.sh --disable-antistall
```

This bounded VM-only experiment synchronizes long-lived targets with two-second
heavy-load and shrink windows. A background `other` layer prevents idle direct
dispatch. The layer retains at least one CPU, uses a high ordinary cross-LLC
migration threshold, and stops other targets plus userspace CPU-mask updates
after the first causally matched target. These post-match controls prevent later
work or configuration changes from masking the stranded state. The race remains
probabilistic, so a run without a target match is inconclusive.

Validated on `scx_layered` v1.1.4 and Linux `6.18.8-1-cachyos`:

- **Default anti-stall:** PID 41750 matched `Q=1, C=0, D=0` and made no CPU
  progress across 248 runnable-state samples. Its enqueue-to-running delay was
  **12.847 seconds**. [Evidence](results/stall-default/monitor.json).
- **Anti-stall disabled:** PID 45841 made no CPU progress across 842
  runnable-state samples. After **43.722 seconds**, the kernel reported
  `runnable task stall`, activated sched_ext bypass, and detached layered. The
  same PID and DSQ have a complete request, clear, empty-recheck, landing, and
  post-watchdog running sequence, with zero dropped probe records.
  [Evidence](results/stall-watchdog/monitor.json).

The watchdog is a kernel-independent confirmation of task-level stall, not a
normal layered rescue. It does not establish a permanent default-configuration
hang. See [the evidence summary](results/README.md) for raw-log locations and
interpretation. `STALL_SECONDS` defaults to 90; `--build-only` performs no load.

## Review Order

1. [Source audit and trusted boundary](docs/audit.md).
2. [Rust scheduler fragments](src/policy.rs).
3. [Local operation language](src/monitor.rs) and [host API bindings](src/api.rs).
4. [Concurrent model, immediate invariant, deferred witness](src/model.rs).
5. [Connection from checked callback receipts](src/composition.rs).
6. [Pinned-source extraction](scripts/evidence.py) and [C/Rust replay](tests/replay.rs).

This follows Lachesis's checked-local-history / concurrent-model proof pattern.
It does **not** register callbacks with the Lachesis runtime or produce a loadable
BPF object. Non-host, non-Verus builds deliberately fail. The C-to-Rust fragment
translation is reviewed and differentially tested, not formally verified.
