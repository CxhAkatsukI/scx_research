# Source Audit and Proof Boundary

## 1. Fixed Evidence

All three source files are fetched and checked against Git blob IDs in
`upstream.json`; extraction fails if the pinned statements cannot be located.

| Evidence | Pin | Relevant code |
| --- | --- | --- |
| scx_layered | `a40cb6115e687a0838c0ccce9b5210b356124f83` | `main.bpf.c`: drain bit helpers 419/428, configuration 598, enqueue tail 2188, drain 2483 |
| scx compatibility | same scx commit | `compat.bpf.h`: 28-46, two-argument move maps to the v1 one-argument kfunc |
| Linux | `v6.16`, `ext.c` blob `7dd5cbcb7a069deea2789a1bad78d5f21770a6d9` | insertion request 2154/6290, callback/commit 2300-2316, actual DSQ insertion/count 1941-2020, move 6566, count read 6960 |

Links: [layered source](https://github.com/sched-ext/scx/blob/a40cb6115e687a0838c0ccce9b5210b356124f83/scheds/rust/scx_layered/src/bpf/main.bpf.c#L2188),
[compatibility](https://github.com/sched-ext/scx/blob/a40cb6115e687a0838c0ccce9b5210b356124f83/scheds/include/scx/compat.bpf.h#L28),
[Linux insertion](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L6290).

This is a pinned **API/fragment pairing**, not evidence that the entire 2026
layered binary has been built or loaded on Linux v6.16. The zero enqueue flags
in this drain call are compatible with the one-argument API through scx's macro.

## 2. What the Actual Statements Do

`Q` is the target **kernel DSQ's queued count**. It is not a scheduler-owned count
of queued plus in-flight work. `C` is `nr_llc_cpus[llc] != 0`; `D` is that LLC's
bit in the layer's per-NUMA-node drain mask.

The configuration path publishes CPU ownership before examining DSQ lengths.
For this slice, that update has already left one old task in an uncovered LLC
with draining enabled. Coverage stays false; no config instruction is interleaved
in the experiment. This isolates retirement versus an in-flight enqueue.

The enqueue tail requests FIFO/vtime insertion, reads coverage, and when it is
zero sets D and invokes the node idle-CPU kick helper. The selected drain
iteration has already passed its D=true, C=false, node/LLC lookup and rotation
guards. It then:

1. Reads Q; if Q <= 1, clears D and remembers `disabled=true`.
2. Attempts one move to the dispatching CPU's local queue.
3. If it disabled D and a second Q read is nonzero, sets D again.

The source comment relies on insertion having acquired/released the DSQ lock
before the enqueue-side coverage check. That is **not** the v6.16 enqueue-call
behavior: `scx_dsq_insert_commit()` calls `mark_direct_dispatch()` and returns.
This records a destination on the task. Only after `ops.enqueue()` returns does
`do_enqueue_task()` invoke `direct_dispatch()` and ultimately `dispatch_enqueue()`.
The latter actually locks the user DSQ, links the task, and increments its count.

`scx_bpf_dsq_nr_queued()` reads the actual DSQ count; it does not include another
CPU's pending insertion request. `scx_bpf_dsq_move_to_local()` flushes **its own
CPU's dispatch buffer**, not another CPU's pending enqueue placement.

## 3. The Concrete Candidate

Start at the selected fragments with `Q=1, C=false, D=true`. Task A is queued;
task B has an earlier placement decision for this LLC but has not requested its
insertion yet. Such a prefix can involve configuration removing coverage after
B's placement decision and enabling draining for A. Reachability from scheduler
initialization is not formally proved here.

| CPU E: enqueue B | CPU R: drain A | Q | D |
| --- | --- | --- | --- |
| request insertion (not committed) | | 1 | true |
| read C=false; set D; invoke kick | | 1 | true |
| callback still executing | read Q=1; clear D | 1 | false |
| | consume A successfully | 0 | false |
| | re-read Q=0; do not repair | 0 | false |
| return from callback | | 0 | false |
| kernel commits B's insertion | | 1 | false |

The two CPUs are different and in the same NUMA node; R can be outside the target
LLC. Choose A with affinity including R and its task rq on R, so consuming A
does not need the enqueue CPU E's rq lock. The user-DSQ lock is acquired only
during the actual queue manipulation, not for E's entire callback. Thus E's rq
lock alone does not serialize these fragments. No preemption of E is needed;
concurrent execution on R is enough. The kick helper need not find an idle CPU;
the model records its invocation, not a guaranteed future dispatch.

The **last state violates the drain-bit implication**, but proving permanent
starvation would require modeling other dispatch/fallback/anti-stall paths and
fairness. Those paths may later rescue B. Do not report this as an upstream-
confirmed bug or a reproduced hang.

## 4. What Verus Checks

`policy.rs` contains the executable Rust translation of the two selected
fragments. Neither callback is external/trusted. Each API/shared operation
appends a ghost receipt through a checked wrapper. The callback postconditions
prove that these receipts obey the language in `monitor.rs` and finish in Done.
There is no precondition asserting the desired shared Q/D safety result.

`model.rs` separates callback operation order from trusted value/lifecycle facts.
Return and Land are distinct events. The deferred model updates Q only at Land;
the immediate control updates it at Insert instead. Reads reflect Q at their
modeled linearization point; a successful consume removes exactly one task.
Failed consume is allowed even with Q>0 in Verus. Host tests choose successful
consumption whenever work is present and eligible.

`composition.rs` derives model-enabled steps from arbitrary merges of the checked
local receipts, plus kernel facts. It does **not** assume the whole merge is a
legal model trace. `verified_callbacks_refine()` consumes the actual checked
`Log` postconditions and works for both insertion modes.
`immediate_callbacks_safe()` derives the terminal safety result under the
explicitly stronger immediate contract. `connected_counterexample()` proves the
deferred witness belongs to both local callback languages and satisfies the
kernel facts, then establishes its completed bad state.

This does not prove that the witness's particular API returns are feasible on a
specific loaded kernel: that remains the source audit and eventual runtime test.
The source extraction + C/Rust differential execution supplies additional
evidence, not a formal C refinement theorem.

## 5. Trusted and Omitted Parts

- Trusted raw host bindings: correct object identity, the chosen kernel API
  effects and callback lifecycle, fresh/current-value reads in the abstract
  model, atomic single-bit projection, no overflow, successful lookup and valid
  DSQ. The real count API is signed and can return errors; this successful slice
  has only 0-2 queued tasks, so the unsigned host representation is exact here.
- SC host primitives strengthen memory semantics for the control experiment.
  The deferred witness needs **no stale reads**: its zero read happens before B
  exists in the DSQ. The immediate safety theorem must not be advertised as a
  Linux weak-memory proof. A stale-zero mutation intentionally invalidates it.
- Coverage and topology are fixed: one selected layer, LLC, NUMA node; other
  drain-mask bits are projected away. The real OR/AND operations preserve those
  bits; the host's single bool projection has the same selected-bit effects.
- FIFO is selected for host replay. The extracted C retains both insertion
  branches; vtime ordering, earlier placement decisions and all full callback
  branches are outside the proof.
- Extraction copies whole bit helpers and the selected source statements. The
  only expression substitution is an instrumented read in place of
  `layer->nr_llc_cpus[llc_id]`; statistics are no-ops. Kernel functions themselves
  are **not** compiled into the host oracle: `oracle.h` supplies their modeled
  effects. Source hashes alone do not validate those effects.
- One enqueue + one drain, no repeated workload arrivals, migration/cancellation,
  concurrent config updates, other consumers, watchdog, bypass, DSQ destruction,
  or proof of end-to-end affinity/progress. Every API boundary is schedulable,
  but instructions internal to a single modeled API are abstracted at its effect.
- No runtime Lachesis registration, BPF build, loader, VM run, or integration
  theorem for the complete upstream scheduler. The checked Rust is a fragment
  translation, not unmodified upstream C.

## 6. Next Validation

Before escalating the candidate, pin a runnable full scx/kernel pair and confirm
the same insertion semantics there. Instrument the real enqueue return/DSQ commit
and drain windows, force E and R onto different CPUs, and test the trace above.
Then include the actual surrounding placement/dispatch guards and rescue paths.
If a repair is pursued, model it against deferred placement explicitly rather
than making Insert atomic-with-accounting or importing mitosis's N by assumption.
