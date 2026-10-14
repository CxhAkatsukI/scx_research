# Problem 1 in Lachesis

Real Rust sched_ext callbacks, a conservative drain protocol, and a conditional
callback-to-model connection proof. No C scheduler callbacks are used.

Read in this order:

1. `src/sched/main.rs`: actual scheduler callbacks.
2. `src/runtime/lib.rs`: checked CAS loops and callback contracts.
3. `src/protocol/word.rs`: the executable packed-word operations.
4. `src/protocol/model.rs`: task lifecycle, including the interrupted ticket windows.
5. `src/trusted/primitives.rs` and `log.rs`: raw bindings and checked receipt wrappers.
6. `src/protocol/composition.rs`: callback receipts enable shared steps; arbitrary interleavings.
7. `src/protocol/connection.rs`: project tracked work onto the queue safety property.
8. `docs/trust.md`: kernel premises, proof boundaries, and validation results.

The first implementation has one partition and one target LLC queue. CPU 0
normally serves it; other CPUs may drain it. `C` is the scheduler's published
ownership flag, not a discovered hardware topology. Affinity is enforced by
the kernel's DSQ move operation; it is not proved here. The theorem is safety,
not eventual service.

## Protocol

One CAS word contains `C` (coverage), `D` (drain), and `N` (registered work).
`Q` is actual work in the target custom DSQ, not a Rust userspace queue.

```text
enqueue: release any removed old registration
         CAS register N (+ enable D if !C)
         store ticket = owned                 <-- NOT atomic with the CAS
         request target DSQ insertion
kernel:  commit insertion later
dispatch: move eligible work to local DSQ; CAS retire D only if N == 0
running/disable/exit_task: release N only after removal/cancellation
config:  CAS publish C (+ enable D if N > 0 && !C)
```

`N` conservatively includes in-flight and detached work. The proofs establish
`Q <= N` and `N > 0 && !C ==> D`, hence `Q > 0 && !C ==> D`.
Other CPUs may run config/drain between the count CAS and ticket store.

## Build And Check

This experiment reuses the existing portable toolchain in `../lachesis_step1`.
It pins Lachesis to `3120c6f` and Verus to `5f6eb59`; it does not modify either.
First prepare that sibling toolchain (`make setup verus` there), then:

```sh
make setup
make check                 # proofs, executable word tests, seven rejected mutants
make all                   # build against the running kernel's BTF
make smoke-loader          # build a bounded partial-switch test loader
```

Requires Rust 1.98 with rust-src/rustc-dev, LLVM tools >= 22 (tested with 23),
libbpf headers, bpftool, and a target kernel with sched_ext and BTF.
The Rust callbacks pass through Lachesis `Policy` / `scheduler!`, ghost erasure,
LLVM CO-RE postprocessing, kfunc BTF fixup, then BPF code generation.
Problem 1 has its own callback contracts; upstream's contracts describe Ipanema.

For a kernel other than the running host, pass its BTF explicitly:

```sh
make vm-smoke VMLINUX=/absolute/path/to/guest/vmlinux.btf
```

VM testing needs `vng` (defaults to the sibling's cached installation), KVM,
and `KERNEL` (defaults to `/boot/vmlinuz-linux`). The guest checks the BTF hash
and refuses to replace an active scheduler. Evidence stays under `build/vm/`.
On a hardware test machine with CPUs 0, 1, 2 available, rebuild against that
machine's BTF, then explicitly run `sudo bash scripts/load.sh`. Only the eight
worker children opt into SCHED_EXT; the controller remains under SCHED_OTHER.
Nothing loads onto the host during `make check` or `make all`.

Generated objects, proof logs, mutants, and dependencies are ignored under
`build/` and `dep/`. Start review with the callbacks, then the trust document.
