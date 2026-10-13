# Problem 1: Published Accounting Across Delayed Queue Insertion

This directory contains the Problem 1 protocol implementation, kernel lifecycle
research, semantic contracts for shared operations, and proofs connecting those
operations to a lifecycle model. Its source is separate from the earlier
reproduction, mocked-adapter, and `lachesis_step1` experiments for easier review.

The key change is: **registration precedes the insertion request, N includes work
that has not landed yet, and C, D, and N are updated through one atomic word.**
Returning from `dsq_insert()` is not treated as actual queue insertion. Neither a
successful consume, a failed consume, nor `quiescent` authorizes a count decrement.

The current scope is two LLCs per partition, with each N limited to 65,535. The BPF
prototype implements one partition and two user DSQs. Configure the CPU-to-LLC
mapping for the target machine before loading. C is the complete two-LLC coverage
configuration, not an implementation of arbitrary CPU masks.

## Review Guide

| File | Contents |
|---|---|
| [docs/kernel-lifecycle.md](docs/kernel-lifecycle.md) | v6.16 call chains, release points, and trusted premises |
| [docs/contracts.md](docs/contracts.md) | Callback obligations, atomic steps, correspondence, and limitations |
| [docs/validation.md](docs/validation.md) | Tool versions, runtime evidence, kernel/BTF identities, and reproduction commands |
| [src/word.rs](src/word.rs) | The single implementation of count and bit operations shared by Rust host and BPF |
| [src/lib.rs](src/lib.rs) | Host callback accounting, task/queue/generation identity checks, and CAS retries |
| [bpf/callbacks.bpf.c](bpf/callbacks.bpf.c) | Kernel-facing struct_ops callback prototype and task storage |
| [verify/](verify/) | Precise bit-operation semantics, an unbounded lifecycle model, and their connecting proofs |
| [tests/](tests/) | Per-task interleaving checks, mock-kernel tests of actual callbacks, and concurrency/boundary tests |

## Local Reproduction

```sh
cd problem1_accounting
make check
make build
# Show interleaving coverage and shortest counterexamples for four broken protocols:
cargo test --test interleavings -- --nocapture
# Optional: fetch and verify the research kernel sources; not part of the model proof:
python3 scripts/fetch_kernel.py
```

`make check` requires Rust/Cargo, Clang 23 with its sanitizer runtime, and Verus.
By default, it reuses `../lachesis_step1/dep/verus/source/target-verus/release/verus`;
override this with `VERUS=/absolute/path/to/verus`. Validation uses stage1's pinned
Verus `5f6eb59`, Rust 1.98.0, and Z3 4.16.0. Host Cargo uses Rust 1.94.1.

Build the BPF ELF offline:

```sh
make bpf VMLINUX=/absolute/path/to/target/vmlinux
```

Raw BTF is also accepted. The build reuses stage1's BPF target and compiled
`libcore`, and requires matching Rust 1.98.0, LLVM/Clang 23, bpftool, and libbpf
development headers. Override tool paths with `BPF_RUSTC`, `BPF_RUST_DEPS`, `CLANG`,
and `BPFTOOL`. Outputs stay in ignored `build/` and `target/` directories. The
checks and offline build commands above do not load the scheduler.

VM checks use QEMU/KVM, virtme-ng, and four vCPUs. They default to stage1's `vng`
and `/boot/vmlinuz-linux`. Override these paths with `VNG` and `KERNEL`:

```sh
make vm-verify  # Run only the verifier; do not attach the scheduler.
# On a BTF mismatch, the command exports guest BTF and exits. Rebuild with it:
make bpf VMLINUX="$PWD/build/vm/vmlinux.btf"
make vm-verify
make vm-smoke  # Attach briefly in QEMU; only eight workers enter SCHED_EXT.
```

VM commands compare guest BTF against the build record. The runtime test maps
vCPUs 0 and 1 to two **synthetic LLC groups**, with the controller on vCPU 2; this
does not represent physical LLC topology. The test has a timeout, detaches on
exit, and checks for the disabled state. Both scripts and the loader refuse to
run outside QEMU.

## Recorded Validation Results

| Layer | Result | Supported conclusion |
|---|---|---|
| Verus | **25 verified, 0 errors**, with `--no-cheating` | Actual Rust word functions satisfy their semantic contracts; the unbounded lifecycle preserves its invariant; word effects preserve correspondence with model steps |
| Negative proof tests | Four faulty variants of actual function bodies each produce proof errors | Contracts detect lost drain requests, overwritten bits, decrements at the wrong position, and ignored pending work |
| Per-task model checking | **2,410 states, 20,920 transitions** | Correspondence and safety hold throughout the finite two-task, two-LLC state space, allowing arbitrarily many re-enqueues and pending cancellation after confirmation that no late commit is possible |
| Protocol counterexamples | All four broken protocols reach a stranded state | Separate request, callback-return, and kernel-commit steps expose windows hidden by a merged model |
| Rust host | Six tests pass | Identity/generation/queue checks, saturation, invalid configuration, concurrent CAS, and the pending window |
| Actual C callbacks + Rust word | Three lifecycle scenarios pass with ASan/UBSan enabled | The same callback bodies behave as expected in a mock kernel with explicit delays, including successful/failed consumption, PID reuse, disable/exit, and re-enqueue |
| BPF ELF | Builds successfully; `cmpxchg` is present; no unresolved Rust/core symbols remain | The actual Rust implementation is linked into the BPF callback object |
| QEMU verifier | Linux **7.2.9-arch1-1** accepts all programs | The object passes the verifier on this test kernel; this check does not attach the scheduler |
| QEMU runtime | Eight SCHED_EXT workers, 1,000 successful configuration updates, 1,000 valid word samples, final N=0, and disabled after detach | Actual callbacks/task storage work under a short workload with migration, sleep/wakeup, re-enqueue, and disable/exit; this does not exhaust concurrent windows |
| Deployment kernel / complete refinement proof | **Not completed** | No claim that all races on the deployment kernel are eliminated or that the complete C callback-to-kernel correspondence is verified |

The final ELF uses BTF exported from the 7.2.9-arch1-1 VM. **The lifecycle research
baseline remains Linux v6.16**; these are not treated as the same target. See the
validation record for runtime logs and object identities. The host is WSL
6.6.87.2; no BPF scheduler was loaded on the host.

## Remaining Integration Work

Before deployment, pin the target kernel commit/BTF, recheck task locks and safe
release events, configure real LLC topology, rerun VM checks on the target
version, and collect per-task evidence for pending and cancellation/migration
windows. Current VM stress sampling does not observe each task's KernelCommit
or force a particular pending window. The C callback's CAS/task-storage binding
remains trusted. Formal connecting proofs cover word effects and the abstract
lifecycle, not complete C callback control flow or Linux itself.

This version proves safety properties. Conservative Detached registrations may
remain until a later safe callback. Timely drain retirement, CPU wakeup, eventual
task execution, and fairness across multiple partitions are not proved.
