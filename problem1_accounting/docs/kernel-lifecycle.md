# Kernel Evidence and Task Registration Release

This research is pinned to upstream **Linux v6.16** as the reference semantics
for trusted API specifications. It is not a statement about the kernel version
on the machine where the scheduler is loaded. `scripts/fetch_kernel.py` downloads
complete source files through the GitHub API and verifies their Git blob SHA-1
identities, including the Git blob header:

| File | Blob SHA-1 |
|---|---|
| `kernel/sched/ext.c` | `7dd5cbcb7a069deea2789a1bad78d5f21770a6d9` |
| `kernel/sched/core.c` | `81c6df746df17b88b1d5bc5a61c5c9d98e07084f` |
| `kernel/sched/syscalls.c` | `547c1f05b667ef1a381989bb984015c7c3838344` |

Line numbers in the links below are the actual line numbers in these downloads.

## Insertion Request and Actual Commit

In enqueue, `scx_bpf_dsq_insert()` goes through `scx_dsq_insert_commit()` to record
a direct-dispatch decision and call `mark_direct_dispatch()`, rather than
immediately modifying the user DSQ. After the callback returns,
`do_enqueue_task()` handles `direct_dispatch()`. Then `dispatch_enqueue()` links
the task into the queue and updates the DSQ count.

Evidence: [request path](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L6290),
[branch after callback return](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2298),
[direct dispatch](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2184),
[actual DSQ modification](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L1941).

The model therefore separates
`Register -> RequestInsert -> CallbackReturn -> KernelCommit`. N increases at
Register's CAS and covers the entire pending window. In a dispatch callback,
insertion may instead enter a dispatch buffer. This prototype issues no new
user-DSQ insertion requests from dispatch, avoiding a single spec that merges
these two API paths.

## Why Not Pair Registration with Dequeue / Quiescent

The `SCX_ENQ_CLEAR_OPSS` branch in `dispatch_enqueue()` clears ops state.
`ops_dequeue()` calls `ops.dequeue` only in a particular `SCX_OPSS_QUEUED` branch.
Thus, "every departure from a DSQ triggers dequeue" is not a valid premise.

More importantly, `dequeue_task_scx()` calls `quiescent` before its final call to
`dispatch_dequeue()`. Releasing in quiescent could remove accounting while the
task remains in a user DSQ. A dequeue during migration may also skip quiescent.

Evidence: [clearing ops state](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2035),
[conditional dequeue branch](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2412),
[quiescent and unlink ordering](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2459).

## Conservative Release Events

| Event | Why the old registration can be released | Implementation behavior |
|---|---|---|
| `running(p)` | `set_next_task_scx()` handles `ops_dequeue` and `dispatch_dequeue` before calling running; this generation has left the original user DSQ | Release the owned ticket in this task's storage |
| Re-enqueue via `enqueue(p)` | The same task's old enqueue has ended and been removed; rq locking prevents an old direct-dispatch commit from crossing this call | Release the old ticket, register a new generation for the new target, then request insertion |
| `disable(p)` | A class switch first dequeues through the old class; `switched_from_scx()` then calls disable | Release; repeated disable/exit does not decrement again |
| `exit_task(p)` | An aborted fork may never have registered; on normal destruction or scheduler disable, the old queueing obligation has ended | Release if owned; delete task storage after success |
| Successful move to a local DSQ | The task has left the user DSQ, but the bool return does not identify it; migration paths may omit notifications | **Do not release**; defer to later running/safe-removal handling |
| Cancellation, migration, quiescent | The callback may precede actual unlink, or a notification may be skipped | **Retain** the registration until a later safe event |

Evidence: [removal before running](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L3246),
[dequeue and callbacks during class switch](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/core.c#L7242),
[sched_setscheduler path](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/syscalls.c#L703),
[switched_from](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L4054),
[disable / exit_task](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L3880),
[synchronized cleanup on scheduler exit](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L4937).

This evidence supports the choice of safe release points; **it is not a
machine-checked proof of all call paths in the target kernel**. Task-callback
serialization, the impossibility of an old generation landing again, and the
lifetime binding between task storage and task objects are explicit premises of
the current trusted adapter. Recheck them when the target version changes.

## Meaning and Cost of N

N includes registered but uncommitted work, actually queued work, work moved to a
local DSQ, and removed tasks with no safe release notification yet. The latter
two categories are Detached. Thus, the accounting obligation is not `N == Q`,
but `Q <= N`, with each registration released exactly once at a confirmed safe
callback.

A task that remains asleep after cancellation may retain a ticket until
re-enqueue, disable, or exit. This conservatively keeps D enabled and may reduce
efficiency, but does not hide actually queued work. When the counter is full, a
new enqueue takes the kernel GLOBAL DSQ fallback before issuing a user-DSQ
request. Safety does not depend on the count eventually reaching zero, nor does
the model assume a release callback that the implementation does not provide.

Task identity uses task storage, not PIDs. Generation numbers increase
monotonically, and overflow is rejected. The host implementation additionally
uses non-reused partition/task IDs to detect releases for the wrong task, wrong
queue, or stale generation. BPF has one partition; the current task pointer and
its private storage provide identity, and there is no asynchronous release entry
point accepting arbitrary old tickets.
