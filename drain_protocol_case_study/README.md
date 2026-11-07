# Real Drain Protocol Case Study

First case: the historical `scx_mitosis` lost-drain race fixed by
[scx PR #3760](https://github.com/sched-ext/scx/pull/3760).
This studies the upstream protocol, not our earlier packed-word protocol.

## Scope

The implemented slice is enqueue accounting racing with **empty-queue drain
retirement**, for one selected subcell/LLC after its coverage has become false.
It includes real-source C execution, a corresponding verified Rust fragment,
and a Lachesis-style local-receipt/merge proof.

**This is not a complete mitosis port, a loadable Lachesis scheduler, or a VM
reproduction.** The Lachesis method and existing Verus toolchain are reused;
the Lachesis runtime is not linked. Shared-read coherence is an explicit,
strong assumption, not a proved Linux memory-model result.
See [scope and trust boundaries](docs/scope.md) before interpreting the results.

## Results

- Verus: **34 verified, 0 errors**. The historical version has a legal completed
  counterexample. The fixed fragment preserves its invariant for arbitrary
  finite merges of any number of participating calls, under stated assumptions.
- Host exploration, one enqueue and one retirement: **9 historical schedules,
  2 bad; 60 fixed schedules, 0 bad**. These are primitive-operation schedules,
  not all possible kernel executions or weak-memory behaviors.
- The Rust and pinned C fragments have identical receipts, read results, and
  final states on every explored schedule.
- Four negative checks are rejected by Verus: read coverage before incrementing,
  omit the repair recheck, omit the repair kick, and permit stale zero reads.
  Kick rejection checks the operation contract, not a liveness theorem.

## Run

```sh
cd drain_protocol_case_study
make check
```

Requirements: Python 3, a GNU-compatible C compiler, `rg`, and a matching
Verus/Rust installation. Defaults reuse `../lachesis_step1/dep/verus` and
Rust 1.98.0. Tested with Verus `0.2026.09.06.5f6eb59` and GCC 16.2.1.
For a different installation:

```sh
make check VERUS_ROOT=/absolute/path/to/verus/release RUSTC=/absolute/path/to/rustc
```

`make fetch` obtains three pinned source blobs and checks their Git object
hashes. It uses `dep/scx` when available, otherwise the GitHub API. No full
checkout is required. `make clean` removes generated `build/` files only.

## Review Order

1. [Source pins](upstream.json) and [scope](docs/scope.md): source locations,
   the race, assumptions, and excluded paths.
2. [Rust fragment](src/policy.rs): the source-level operations being checked.
3. [Local monitor](src/monitor.rs) and [primitive bindings](src/primitives.rs):
   receipt contracts and the small trusted machine boundary.
4. [Concurrent model](src/model.rs): accounting credits, repair debt, invariant.
5. [Composition](src/composition.rs): derive model steps from checked logs,
   then prove safety; explicit old and repaired witnesses are at the end.
6. [Replay](tests/replay.rs), [C extraction](scripts/extract.py), and
   [negative checks](scripts/mutants.py): independently run the relevant code.

All work is isolated here; upstream data and generated artifacts are ignored.
The next extension is the nonempty/consume branch and a real Lachesis kernel
backend. Neither is claimed complete by this first case study.

## Layered Companion Case

The independent [layered study](layered/README.md) audits kernel DSQ visibility
and drain retirement. Unlike the mitosis case above, it does not use logical
in-flight accounting. It includes a conditional deferred-placement counterexample
and an explicit fragment-level proof boundary.
