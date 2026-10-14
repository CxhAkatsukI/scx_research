# Proof Boundary

## What Is Checked

| File | Verus obligation |
| --- | --- |
| `protocol/word.rs` | Actual executable register/release/publish/retire arithmetic and bit preservation |
| `trusted/log.rs` | Receipt append and prefix certificates; no trusted proof bodies |
| `trusted/primitives.rs` | Public wrappers append the actual call arguments/results; raw FFI bodies remain trusted |
| `runtime/lib.rs` | Actual bounded CAS retry loop, ticket release order, and callback contracts |
| `sched/main.rs` | Actual Rust callback bodies satisfy those contracts |
| `protocol/model.rs` | Each enabled fine-grained step preserves exact registration accounting and drain safety |
| `protocol/composition.rs` | Checked local receipts + primitive/lifecycle premises imply enabled model steps and preserve the coupling |
| `protocol/connection.rs` | `Q <= N`, hence registered-work safety implies target-DSQ safety |

In `composition.rs`, start with `receipt_enables_model`, `checked_receipt_refines`,
`event_refines`, then `checked_callbacks_are_safe`. The latter separates
`callback_order` (the checked receipt order/return contracts) from
`kernel_execution` (external primitive and lifecycle facts). Neither primitive
coherence nor the kernel predicate assumes `model::enabled` or the drain invariant.
`Log::wf` and `checked_prefix` certify each operation, including paused callbacks.

The induction covers any finite event sequence and any finite number of task and
callback slots, not a few example permutations. Task slots are logical identities,
not PIDs. At most 65535 registrations fit into the executable packed counter;
new work falls back to the kernel global DSQ at capacity/allocation/retry failure.

## The Ticket Window

```text
Off -> Reserved -> Owned -> Requested -> Queued -> Detached -> Releasing -> Off
       count++     ticket   insertion    kernel    kernel      count--     ticket
                   true     requested    landing   unlink                 false
```

`Reserved` already contributes to `N`, although the physical ticket is still
false. `Releasing` no longer contributes, although the ticket is still true.
Both windows allow arbitrary config/drain interleaving. Same-task callbacks
cannot enter those windows concurrently. `reserved_window_is_admitted` checks a
concrete config/retire interruption followed by ticket publication and landing;
the preservation theorem is general, not limited to that witness.

Removing a task from the custom DSQ does not immediately release its registration.
It stays `Detached` until running, disable, exit_task, or the next enqueue releases
it. `dsq_move_to_local` returns a boolean, not the moved task's identity; dispatch
therefore never guesses which ticket to decrement. Cancellation is a separate
kernel event which guarantees no late landing of the cancelled request.

## Explicit Trusted Premises

1. **Primitive interpretation.** One successful word CAS is linearizable, with
   its real observed/before/after values. Failure does not modify the word.
   Every shared-word writer uses the checked update code. The initial word is 1.
   Snapshot can be stale; it is never treated as an atomic read-modify-write.
2. **Task storage.** Lookup returns the same live task's stable, initially-zero
   storage; it does not alias another task. Plain ticket accesses have same-task
   serialization. Numeric PID reuse does not reuse a live logical registration.
3. **Same-task lifecycle.** Before one enqueue finishes, the same task cannot
   re-enter enqueue or another callback which releases/rewrites its ticket.
   A callback that releases an old ticket begins only after the old target-DSQ
   membership/request has been removed/cancelled and cannot later land again.
   This does NOT exclude other tasks or other CPUs executing config/dispatch.
4. **DSQ lifecycle.** Successful init creates TARGET before enqueue/dispatch.
   Target insertion requests eventually land at most once or are cancelled.
   Kernel commits/unlinks correspond to Land/Remove/Cancel with the same identity.
   Kernel fast paths never insert unregistered work into this custom target DSQ.
   They may use built-in local/global queues, which are outside this `Q`.
5. **Invocation and ABI.** Trampolines pass the correct task/context. Per-callback
   receipt order is preserved when primitive events are merged at their
   linearization points. Kernel callbacks and storage destruction obey premise 3.
6. **Toolchain.** Verus, Rust ghost erasure, LLVM/BPF compilation, CO-RE relocation,
   and the kernel verifier preserve the relevant source behavior.

These premises are NOT proved about Linux by this experiment. In particular,
raw `external_body` functions do not contain a formally instantiated concurrent
heap/atomic invariant. `primitive_coherent` and kernel events are the explicit
trusted interpretation of the executable FFI operations. The verified wrappers
and connection lemmas check the scheduler side of that interpretation; they do
not automatically extract a Linux execution trace or prove FFI memory semantics.
Thus the result is a conditional trace-refinement proof, not an unconditional
end-to-end theorem about a particular kernel binary.

Reference source inspected: Linux v6.16 `kernel/sched/ext.c`, especially
`do_enqueue_task`, `mark_direct_dispatch`, `direct_dispatch`, `dispatch_enqueue`,
`set_next_task_scx`, `scx_disable_task`, and `scx_exit_task`:
<https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c>.
In this path, insertion is requested in enqueue and committed after callback
return. The model deliberately permits later landing rather than pretending
`dsq_insert` synchronously increments `Q`. This reference is NOT a source audit
of the newer VM kernel; its lifecycle premises remain an explicit audit item.

## Deliberate Limitations

- One partition and one target LLC queue. `C` is a single logical ownership bit;
  there is no verified multiword CPU-mask publication or topology discovery.
- Safety only: no kick/fairness/progress theorem. Enabling D does not by itself
  guarantee an eligible CPU will run dispatch. Affinity and task execution are
  delegated to the kernel, not established by these model lemmas.
- This is a new, conservative packed-word protocol, not a proof of the original
  layered/mitosis drain-disable implementation.
- CAS loops retry at most 32 times. Release failure leaves a conservative count
  outstanding. If a task exits then, its logical registration remains a tombstone
  in the model. This may leak counts/drain requests and cause global fallback;
  it cannot make `N` undercount `Q`. No zero-leak or cleanup liveness claim.
- Receipt logs are ghost-only. They are not production tracing and add no
  event buffer at runtime. The checked scheduler uses only the public wrappers;
  raw calls and receipt append are private to the boundary crate.
- No automatic scheduler extractor or reusable arbitrary-scheduler proof exists.

## Validation

`make check` verifies all four crates, exercises all 262144 packed words using
the erased executable functions, and requires seven mutants to fail verification:
missing enqueue count, missing ticket write, missing release count, missing drain
enable, unsafe drain retirement, unaccounted Reserved work, overlapping same-task
callbacks. Compiler/type errors do not count as successful mutant rejection.

The BPF object was loaded and exercised in QEMU on Linux 7.2.9-arch1-1, using that
guest's BTF. The partial-switch test used eight workers pinned to CPUs 0/1 (with
affinity changes), sleep/wakeup, yielding, disable/exit, and 1000 config updates.
Samples satisfied packed-word safety; all workers progressed; the final count
was zero and the scheduler detached. This is bounded integration evidence,
not an exhaustive test of the trusted premises or a progress proof.
