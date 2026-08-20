# Verus L3 Conversion Notes

This directory is for the Verus-facing conversion of the Problem 1 scheduler
policy core.

Current boundary:

- Included: L3 policy state, policy inputs, policy actions, and Q/C/D state
  transitions.
- Excluded: BPF loading, BPF attach, ring buffers, sysfs topology discovery,
  logging, wall-clock time, sleeps, run-loop shutdown, and errno handling.

The first target is a witness-style check:

```text
there exists an L3 input sequence that reaches Q > 0 && !C && !D
```

For now the witness is constructive: the file gives a concrete input step and
asserts the resulting state. This is simpler than encoding an existential
quantifier over arbitrary input sequences.

Run the first draft as a standalone Verus file:

```sh
verus problem1_l3_step1.rs
```

Alternatively, if you remove the empty `main`, run it as a library-style input:

```sh
verus --crate-type=lib problem1_l3_step1.rs
```

`problem1_l3_step2_interleaving.rs` is the next draft. It keeps the same Q/C/D
state but models the bug as small-step interleaving between enqueue and config
update:

```text
EnqueueSelect -> ConfigPublishMask -> ConfigObserveQueue -> EnqueueCommit
```

`problem1_l3_step3_fixed_interleaving.rs` sketches a fixed small-step protocol.
It splits enqueue into select/insert/repair and drain retirement into
disable/consume/repair. It proves a protocol invariant is preserved by every
single interleaving step, then lifts that to arbitrary finite traces with a
recursive `run_steps` lemma. The final safety statement is conditional on the
trace ending in a stable/completed state, because transient bad states are
allowed while a path still owes repair.
