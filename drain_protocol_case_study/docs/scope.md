# Scope and Evidence

## Pinned Source

[PR #3760](https://github.com/sched-ext/scx/pull/3760) documents a historical
lost-wakeup bug and its repair. This is a regression case, not a newly discovered
upstream bug.

- Before: `1537b3ba6d6b7f5f679973a0c1d15df661b5272c`.
- Fixed: `ef5e1c2ffa2576ff49984e7e0ff53d5eef0bfd0d`, the PR's fix commit.
- [Original empty branch](https://github.com/sched-ext/scx/blob/1537b3ba6d6b7f5f679973a0c1d15df661b5272c/scheds/rust/scx_mitosis/src/bpf/llc_aware.bpf.h#L389).
- [Fixed empty branch](https://github.com/sched-ext/scx/blob/ef5e1c2ffa2576ff49984e7e0ff53d5eef0bfd0d/scheds/rust/scx_mitosis/src/bpf/llc_aware.bpf.h#L389).
- [Enqueue accounting](https://github.com/sched-ext/scx/blob/ef5e1c2ffa2576ff49984e7e0ff53d5eef0bfd0d/scheds/rust/scx_mitosis/src/bpf/llc_aware.bpf.h#L287).
- [Real enqueue caller](https://github.com/sched-ext/scx/blob/ef5e1c2ffa2576ff49984e7e0ff53d5eef0bfd0d/scheds/rust/scx_mitosis/src/bpf/mitosis.bpf.c#L834).

`upstream.json` records file identities, including their Git blob hashes.
Fetched files go to `dep/evidence`; they are not additional handwritten models.

## State and Translation

| Source fact | Rust/model representation |
| --- | --- |
| `subcell->llcs[llc].nr_queued` | `Shared.nr_queued` / `State.n`, abbreviated N |
| `llcs_with_cpus & (1 << llc)` | `Shared.llcs_with_cpus`, frozen C = false |
| `llcs_to_drain & (1 << llc)` | `Shared.llcs_to_drain` / `State.drain`, D |
| One accounting increment | One ghost credit owned by a participating call |
| Task becomes visible after callback return | A separate `Land` event; Q = `landed.len()` |

N is **scheduler-owned logical accounting**, not the kernel's DSQ length.
The actual C enqueue requests `scx_bpf_dsq_insert_vtime()` before calling the
accounting helper. Requesting insertion is not modeled as immediate landing.
The source itself documents the callback-return visibility gap at lines 156-161.

`policy.rs` translates only the successful accounting helper and the empty-count
branch of `try_draining_work()`. It does not replace them with a high-level
Problem1 API. Its wrappers correspond to atomic add, load, OR, AND, and invocation
of the existing kick helper. A constant `fixed` selects the before/after fragment.
Lookups and the drain candidate selection have already succeeded in this slice.

## Historical Execution

Start with C = false, N = 0, D = true. E is one enqueue accounting call and R
is a drain attempt that has selected this orphaned LLC.

| Step | Operation | N | D |
| --- | --- | --- | --- |
| 1 | R reads N = 0 | 0 | true |
| 2 | E increments N | 1 | true |
| 3 | E reads C = false | 1 | true |
| 4 | E atomically enables D; the bit was already set | 1 | true |
| 5 | E calls the kick helper, then returns; its task may land | 1 | true |
| 6 | R clears D using its earlier empty observation, then returns | 1 | false |

The completed state has Q > 0, !C, !D. The prior kick does not restore D;
this proof does not assert that it guarantees any eventual CPU execution.

The repair adds, after step 6: read N again, then enable D and invoke the kick
helper if N is nonzero. If that recheck precedes the enqueue increment instead,
the enqueue still has its own later enable operation. Every primitive is a
possible interleaving boundary; the whole helper is never assumed atomic.

The initial empty-but-enabled state is a **conditional fragment starting point**,
not a theorem about reachability from mitosis initialization. The source permits
conservative drain bits; for example, two drainers can both read N = 2 before
each consumes a task, so neither takes the last-task disable branch. This is a
source-level explanation, not a verified prefix of our model. The PR independently
confirms the historical race. Configuration publication and candidate selection
are not included in this experiment's trace.

## What Is Proved

`monitor.rs` checks local order. For the accounting path:

```text
increment -> read C -> [enable -> kick] -> done
```

For fixed empty retirement:

```text
read N -> [clear -> read N -> [enable -> kick]] -> done
```

Brackets denote branches controlled by the actual read results. `policy.rs`
is verified against these contracts. Its log history is ghost state and becomes
a zero-sized monitor in the host build; it does not mutate the real protocol.

The shared model has any finite number of participating accounting/retirement
calls. Each accounting call increments once; no consumption or configuration
change occurs during the modeled interval. Its invariant is:

```text
N = number of credits
landed tasks are a subset of credited, returned calls
N > 0 and !D => at least one unfinished call still owes a repair
```

`Counted`, `Enable`, `Recheck`, and `Repair` are repair-debt positions. A temporary
N > 0, !D is permitted while one of these operations remains pending. Once all
participating calls finish, debt is impossible, giving N > 0 => D and thus
Q > 0 && !C => D. This is safety at completed states, not termination or progress.

`composition.rs` connects these two levels:

1. Checked helper postconditions export `follows(start, history, Done)`.
2. A certificate is this actual local history, including read values.
3. Any merge preserving certificate order is considered; API invocations from
   different calls may interleave between every primitive.
4. Given shared-object/read coherence and lifecycle facts, the merge proof
   derives `enabled` and invariant preservation. It does **not** assume that
   the callback already follows the model's allowed shared-state transitions.
5. `verified_logs_completed_trace_is_safe` is the main conditional theorem.
   A concrete repaired merge also satisfies its premises and lands one task.

The proof covers arbitrary finite merges in this restricted model. The host test
separately exhausts only one E and one R, finding 9 old schedules (2 bad) and 60
fixed schedules (0 bad). Source C and Rust agree on each explored schedule.

## Trust Boundaries and Remaining Gaps

| Boundary | Current status |
| --- | --- |
| Local Rust control flow -> local receipt contract | Verified, including branch read values |
| Receipt merge -> shared invariant -> completed safety | Verified under the explicit premises below |
| Primitive -> receipt and correct shared object | Trusted bindings and `primitive_fact`, not a kernel proof |
| Pinned C -> Rust fragment equivalence | Source review plus bounded execution comparison; not formally proved |
| Actual kernel execution -> coherent merged trace | Assumed for this experiment; not established for a target kernel |
| Whole scheduler, all partitions, all config/consume paths | Not covered |
| Eventual task service, successful CPU kick, affinity | Not covered |

The important assumptions are:

- **Current-value reads / SC-style schedules.** `Pending(n)` must equal the
  current modeled N. Coverage stays false and reads as false. Linux `READ_ONCE`
  alone is not a proof of this property; neither relaxed Rust loads nor full
  barriers are verified here under LKMM. Permitting stale zero breaks the proof,
  as the negative test demonstrates. A Linux weak-memory argument remains work.
- **Primitive identity and effects.** All selected calls address the same
  accounting cell and drain bit. The RMW operations take effect atomically and
  in per-call order; logs refer to their actual linearization order. No hidden
  writer or decrement changes these fields during this interval. N does not
  overflow `u32`. These are explicit trace premises, not derived from the log.
- **Task/landing lifecycle.** Each participating accounting call represents
  one distinct pending insertion. At most one landing is assigned to it, only
  after its helper has returned. No cancellation, reassignment, repeated enqueue
  of the same task, or consumption is included. `Return` denotes return from
  the translated helper; actual callback return may be later. Real DSQ membership
  and task serialization are not proved by the ghost credit accounting.
- **Path entry.** LLC awareness is enabled, indices and lookups are valid, the
  drain mask selected this candidate, and the drainer is outside the target LLC.
  Coverage publication has completed and is frozen false. Multiple-bit bitmap
  interactions and actual topology discovery are outside the selected projection.

`tests/oracle.h` supplies success-path lookups and instrumented host primitives.
`scripts/extract.py` copies five unchanged source functions plus the unchanged
empty branch into generated C. It is a pinned fixture extractor, not a generic
C-to-Rust or concurrency extractor. The replay scheduler controls the actual C
and Rust calls at primitive boundaries; it does not execute the hand model.

The kick shim records entry to `kick_subcell_idle_cpu()`. The real helper can
fail to find an idle CPU. Removing a required kick fails the local contract,
but this experiment does not prove that the remaining kicks ensure liveness.
The PR also adds a kick in the nonempty consume branch; that branch is excluded.

Finally, this borrows Lachesis's proof structure and toolchain, **not its linked
kernel runtime**. There is no BPF backend, struct-ops wrapper, loader, or loading
script for this fragment. Ordinary compilation without the host-replay or Verus
mode is rejected so the stub cannot silently become a supposed kernel backend.
A real Lachesis integration must implement the machine bindings and establish
the kernel-to-trace assumptions before claiming a loaded-scheduler result.
