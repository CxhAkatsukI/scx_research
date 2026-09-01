# Minimal Scheduler Design

The first scheduler is a single-DSQ FIFO scheduler.

```text
init      -> create DSQ 0
enqueue   -> insert the task into DSQ 0 with a fixed time slice
dispatch  -> move one task from DSQ 0 to the current CPU
```

The scheduler policy in `src/lib.rs` is shared by the mock and BPF builds. The BPF crate implements `ScxApi` with real sched-ext calls. The mock implements the same trait with in-memory queues and an event trace.

`bpf/abi_shim.bpf.c` is the trusted ABI boundary. It contains no scheduling decisions. It only provides the BPF callback calling convention, forwards calls to the target kernel's sched-ext kfuncs, and defines the `sched_ext_ops` table that libbpf expects.

The current `ScxApi` contracts are a sequential abstraction. They verify queue
accounting for individual calls, but do not yet model concurrent callbacks,
liveness, or Problem 1. Those require a later concurrent state model.
