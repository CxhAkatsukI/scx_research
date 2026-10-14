# Lachesis: stage 1

Reproduce Kartikeya's Rust -> Verus -> BPF pipeline before adding new protocols.
The scheduler uses one shared virtual-time DSQ and an idle-CPU direct path.
Its policy and contracts are the upstream implementation.

Source: [kkdwvd/lachesis](https://github.com/kkdwvd/lachesis), commit
`91af38a6b1ff72ee324dc056b9f914d65045b3c9`.
This is an ordinary directory in `scx_research`, so cloning the parent includes
the source. Public Verus dependency:
`5f6eb59f7a5527bf5c0c35995a4d6d1c410981a1` (downloaded into ignored `dep/`).
Original SPDX notices and toolchain attribution are retained.

## Build and check

Use x86-64 Linux, Rustup, Git, Python 3 with pip, Make, a C/C++ compiler,
pkg-config, LLVM 22 tools and development libraries, bpftool, and development
packages for libelf, zlib, libzstd, and libbpf. The scripts install Rust 1.98.0,
the pinned Verus source, and Z3 4.16.0. Verus is built with no_std/no_alloc vstd.
The first build downloads dependencies and can take several minutes.

```sh
cd lachesis_step1
make setup
make check
make build
```

`make check` verifies the original policy, then generates two copies in `build/`:
one passes zero to `charge_vtime`, the other returns positive one from `init`.
Both must fail with the corresponding Verus contract diagnostic. Parse errors
and missing tools do not count as a successful rejection. The real policy file
is never edited by this check.

`make build` uses `/sys/kernel/btf/vmlinux`. Build on the target sched_ext kernel,
or pass `VMLINUX=/absolute/path/to/target/vmlinux` for an offline build.
Missing kfunc BTF is an error. The BPF object and loader are
`build/lachesis/lachesis.o` and `build/lachesis/lachesis`.
Use `LLVM_PREFIX=/usr/lib/llvm-22` if LLVM 22 is installed there.
Download mirrors can be supplied through `RUSTUP_DIST_SERVER` and `PIP_INDEX_URL`.

## Load on the test machine

The running kernel needs sched_ext and BTF, plus the kfuncs this snapshot uses
(including `bpf_stream_vprintk` and `bpf_throw`). Having sched_ext alone does not
guarantee compatibility with this prototype. Rebuild using that machine's BTF.

```sh
sudo ./scripts/load.sh --duration 10
```

The loader prints local/shared enqueue and dispatch counters, then detaches.
Use `--duration 0` to keep it running until Ctrl-C. The script refuses to replace
an already active sched_ext scheduler. A successful ELF build is not evidence
that the kernel verifier accepts or successfully runs it.

For an isolated test, install virtme-ng, QEMU, and a static BusyBox, then run
`make vm-run KERNEL=/path/to/bzImage VMLINUX=/path/to/vmlinux`.
Only `build/lachesis/` is shared with write access. `VNG`, `VM_CPUS`, `VM_MEM`,
and `VM_TIMEOUT` can override the VM tools and resources.

## Recorded result (2026-09-10)

- Rust 1.98.0; Verus `0.2026.09.06.5f6eb59`; LLVM 22.1.8; Z3 4.16.0.
- `make check`: trusted crate 4, runtime 12, policy 12, control 2 verified;
  all with zero errors. Both policy mutations failed for the expected contract.
  The trusted-crate count does not mean external bodies were proved.
- BPF ELF and loader built using BTF extracted from Linux `7.1.11-arch1-1`.
- QEMU/KVM, 4 CPUs: kernel verification passed, scheduler attached, shared-queue
  work was serviced, and the loader detached after 5 seconds with exit kind 64,
  code 0, `state=disabled`, and `nr_rejected=0`.
- Kernel log reports deprecated direct writes to `p->scx.slice/dsq_vtime` in
  the inherited task wrappers. Loading succeeded; migration to the replacement
  setters is deferred so this baseline preserves the original policy/runtime.

Full local logs are ignored build artifacts: `build/check.log`,
`build/lachesis/verifier.log`, and `build/lachesis/run.log`.
This short VM run is a smoke test, not a fairness or concurrency proof.

## Review order

1. `src/sched/bpf/main.rs`: the actual kernel scheduling policy.
2. `src/runtime/policy.rs`: shared callback contracts, inherited by the impl.
3. `src/runtime/vtime.rs`: verified arithmetic, including nonzero weight.
4. `src/runtime/trusted/`: task views, kfunc wrappers, atomics, and BPF entries.
5. `src/toolchain/rules.mk`: host verification and separate erased BPF build.
6. `src/loader/main.rs`: unverified libbpf loader; `src/sched/control/` contains
   its small verified exit-classification helper.
7. `scripts/check-contracts.py`: mutations of the actual policy used for testing.

Current proofs cover the written contracts, assuming the trusted kernel API
specifications. Most DSQ and atomic wrappers have no state-effect contracts.
There is no proof of Problem 1, concurrent accounting, fairness, or progress.
The BPF compiler pipeline and loader remain outside Verus's proof boundary.

## Local changes from upstream

- Replace private submodule and author-specific maintenance targets with pinned
  public setup and short build/load/check commands.
- Default to the running kernel's BTF; locate bpftool through PATH and pin the
  host postprocessor's Rust compiler consistently with the rest of the build.
- Require real BTF prototypes instead of falling back to guessed kfunc types.
- Keep generated files and dependencies out of Git; retain the BPF target JSON
  even though the parent repository ignores JSON globally.
- Replace the private roadmap link and personal agent instructions with local
  stage-1 instructions. The scheduling policy and its contracts are unchanged.
- Keep the upstream VM scripts, locating bpftool through PATH and sharing only
  the output directory with write access.
