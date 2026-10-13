# Semantic Contracts for Callbacks and Shared Operations

## State, Scope, and Correspondence

One partition's atomic word has the following layout. All unused bits must be zero:

```text
63                 36 35            20 19             4 3  2 1  0
+--------------------+----------------+----------------+----+----+
|       zero         |    N[1]:16     |    N[0]:16     | D  | C  |
+--------------------+----------------+----------------+----+----+
```

Every atomic commit preserves `N[l] > 0 && !C[l] ==> D[l]`.
The lifecycle distinguishes Pending, Queued, and Detached, maintaining
`N = Pending + Queued + Detached`. Pending includes the stage after a callback
registers work but before it requests insertion. Detached includes local-DSQ work
and registrations retained after removal.

The correspondence relation binds the model's registered/covered/draining fields
to word fields for **the same partition and the same LLC**. `verify/bridge.rs`
connects actual word effects to Register, Publish, Release, and Retire.
Land/CancelPending/Move/Remove leave word fields unchanged. CancelPending moves
Pending to Detached only after the kernel confirms that this generation's request
cannot land later. Cancellation intent or quiescent alone does not establish that
confirmation. `Q <= N` and the invariant imply `Q > 0 && !C ==> D`, without making
a global concurrent state an individual callback's exit obligation.

The current connecting proof uses an unbounded aggregate lifecycle model.
Per-task identity is covered by finite model checking and runtime tests. A global
correspondence covering all C callback local stages, task storage, and the kernel
has not been formally proved.

## Shared Operations

The following effects occur at **the successful CAS linearization point**. They
describe this operation's contribution, not a guarantee that other CPUs cannot
change fields throughout the call. A failed CAS performs no model step and must
recompute from the word actually returned. BPF returns failure after 32 failed
attempts rather than pretending to succeed.

| Operation | Premises | Atomic effect |
|---|---|---|
| Register(l) | Legal and safe word; `l < 2`; `N[l] < 65535`; the current task has no other owned ticket | `N[l] += 1`; `D[l] := D[l] \|\| !C[l]`; other counts, other D bits, and C are unchanged |
| Publish(c) | Legal and safe word; `c < 4` | Publish complete C at once; for each l, `D[l] := D[l] \|\| (N[l]>0 && !c[l])`; N is unchanged |
| Release(l) | Ticket for the correct task/partition/LLC/generation; this generation is not in a user DSQ and cannot land later; `N[l] > 0` | `N[l] -= 1`; C, D, and other counts are unchanged; the ticket cannot be released again |
| Retire(l) | Legal and safe word; `l < 2` | Clear this D only if `N[l] == 0` in the same snapshot; other fields are unchanged |

The `//%` lines in `src/word.rs` are Verus annotations, treated as comments by the
host compiler. The verification script only activates these annotations, names
return values, and adds specs/proofs; it does not replace algorithm bodies. BPF
compiles **the same source** through `bpf/core.rs` and links it with the C
callbacks' LLVM bitcode. There is no separate hand-written C bit algorithm.
Negative proof tests directly mutate these actual function bodies.

## Callback Obligations and Execution Stages

**Enqueue:** Entry requires a valid task/context, serialized lifecycle operations
for the same task, and removal of the old generation. If the old ticket is still
owned, release it at this safe point first. Then choose a valid LLC, successfully
Register, write the current task's `(LLC,generation,owned)`, and finally issue one
user-DSQ insertion request. The exit obligation binds the request to its
registration; **it does not guarantee that the task is already Queued**. A full
counter or exhausted CAS retries takes the GLOBAL fallback before any user-DSQ
request. No select_cpu path inserts into a user DSQ while bypassing registration.

The local stage between Register's CAS and the task-storage write is Reserved:
N has increased, but the kernel cannot land the task yet. Serialization of the
same task prevents another release path from using its incomplete record during
this window. There is also a local stage between Release's CAS and clearing
owned. The callback clears owned before returning; other tasks do not use that
record. **This local-stage mapping is currently a trusted adapter obligation;
complete C control-flow verification is still missing.**

**Config:** BPF `p1_config` accepts a complete u64 coverage mask in the range 0..3.
One Publish CAS publishes the configuration and all necessary D bits together.
Failure returns `-EAGAIN`; invalid input returns `-EINVAL`. The trusted
configuration writer must call this syscall BPF program, not directly modify the
packed word in `.data`. There is no separate CPU-mask map, so this does not claim
to solve atomic publication of coverage together with arbitrary external masks.

**Dispatch/drain:** Attempt a move only when the LLC is the current CPU's covered
LLC or the target LLC's D is enabled. A successful move transfers one specific
task in the kernel; the caller receives only a bool and does not use it to
release an arbitrary ticket. Failure does not imply Q=0. After either outcome,
drain retirement must use Retire to check N. D cannot remain cleared while
pending work exists. Failed Retire retries leave the existing state intact for
a later retry.

**Running / disable / exit / re-enqueue:** Release only the owned ticket in the
current task's private storage. Clear owned after a successful decrement; a
repeated callback does not decrement again. Do not release without confirmation
of removal. The BPF safe points are based on kernel-version research. The host
API's `removed` argument is explicit caller testimony, not a kernel fact observed
by the library itself.

## Memory Ordering and Trusted Boundary

Host CAS uses Relaxed ordering. The read-modify-write order on this **single
atomic location** is sufficient for the word invariant; it does not require
sequential consistency across independent locations. It does not publish task
memory or synchronize with DSQ locks. The C adapter uses
`__sync_val_compare_and_swap`, including for atomic reads. Offline disassembly
checks that `cmpxchg` is generated. Host Relaxed reasoning is not automatically
a complete compiler-correctness proof for Rust -> LLVM -> BPF -> target architecture.

Additional trusted premises are: successful registration precedes the insertion
kfunc; all insertions into this partition's user DSQs go through registration;
DSQ IDs are not destroyed or reused; the kernel correctly serializes task hooks
and storage; a generation never lands again after safe release; consumption
transfers a task only once; and the word is initialized to a legal N=0/D=0 value
before loading. Valid tasks, CPUs, affinity, and the target kernel version come
from the environment, not facts Verus automatically establishes for a real machine.

## Properties Not Claimed

The current results do not prove the kernel itself, complete C callbacks,
per-task correspondence over an unbounded identity space, freedom from
starvation, bounded CAS waiting, timely drain retirement, or CPU-kick liveness.
The two-LLC packed layout does not directly generalize to arbitrarily many LLCs.
Configuration updates across multiple partitions are not one atomic transaction.

Applying the conditional safety result to a real execution requires checking
the target version and establishing the complete adapter correspondence. VM
verifier acceptance and short runtime tests do not replace that step.
