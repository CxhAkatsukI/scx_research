# Validation Record (2026-10-11)

## Sources, Tools, and Evidence Locations

Generated artifacts remain in ignored `build/` and `target/` directories.
Dependency sources, generated skeletons, ELF files, and large logs are not
included in the delivered source. Earlier experiment directories were not changed.

| Purpose | Version used |
|---|---|
| Lifecycle research | Linux v6.16; see kernel-lifecycle for Git blob identities of the three complete source files |
| Host | WSL `6.6.87.2-microsoft-standard-WSL2`; Rust 1.94.1 |
| Verus | `5f6eb59`; Rust 1.98.0; Z3 4.16.0; `--no-cheating` |
| BPF | Rust 1.98.0 + LLVM/Clang 23.1.1; linked with C bitcode to produce the BPF ELF |
| VM | QEMU/KVM + virtme-ng; four vCPUs, 2 GiB; `7.2.9-arch1-1` |

The VM boots the local `/boot/vmlinuz-linux`. This is a specific test build, not
Linux v6.16 or the project's future deployment kernel. Its SHA-256 identities are:

```text
kernel image:
d02afb36d5f99390fdd8d04dedf8894354f4bbdc5e7d44afa5b897639bb3707c
guest raw BTF:
e707a907dffccbfd0e3e6caa1ab7c9bec6d7f1466235f4ce4e217837cb95fc42
```

`build/bpf/build-info.txt` records the inputs, raw BTF, and object hashes. The VM
runner requires the object to still match this record and the guest BTF to match
as well; it stops on any mismatch. For ELF inputs, `.BTF` is extracted before
comparison. `vm-smoke` regenerates the loader each time to avoid stale skeletons.

## Completed Checks

```sh
make check
make build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --test interleavings -- --nocapture
make bpf VMLINUX="$PWD/build/vm/vmlinux.btf"
make vm-verify
make vm-smoke
```

`make check` includes eight Rust tests, three mock-kernel lifecycle scenarios
executing actual C callbacks, Verus verification of actual Rust function bodies,
and four negative proof tests. The mock-kernel C path enables ASan/UBSan; the
linked Rust object has no sanitizer instrumentation. The per-task model has
2,410 reachable states and 20,920 transitions, with separate request/return/commit
steps and explicit cancellation testimony. It does not explore arbitrary numbers
of tasks or CPUs, or arbitrary kernel behavior.

Verus reports `25 verified, 0 errors`. Each of the four faulty function bodies
produces proof errors. Missing tools, syntax errors, and type errors do not count
as valid negative tests. Logs are in `build/verify/`.

VM verifier logs are in `build/vm/verifier.log`; the check confirms that the
scheduler was not enabled. Runtime test logs are in `build/vm/smoke.log` and
`smoke-run.log`. Only workers enter SCHED_EXT; the controller remains on CFS.
The test checks that sched_ext is disabled after exit.

The runtime test has eight workers performing computation, yield, and
sleep/wakeup, with one worker repeatedly migrating. The controller submits 1,000
coverage updates through the actual `p1_config` BPF syscall program. Each sample
only reads the packed word atomically and checks its legality and
`N>0 && !C ==> D`. The test requires nonzero N and D to be observed, all workers
to make progress and leave SCHED_EXT normally, final N=0, an enabled scheduler
before detach, and a disabled scheduler afterward. Sampling never writes Q, C,
D, or N.

The successful run produced the output below. Progress counts and sample
distribution vary between runs:

```text
OK: workers=8 progress=258423 config_updates=1000 samples=1000 busy_samples=874 drain_samples=818 final_N=0
OK: scheduler detached; sched_ext state=disabled
```

## Result Boundaries

Verifier acceptance does not establish protocol correctness, and short sampling
does not exhaust interleavings. The VM does not directly record each task's
pending/KernelCommit events or use actual DSQ Q to check the complete per-task
correspondence. The proofs cover word functions, the unbounded aggregate
lifecycle, and their correspondence. The kernel, complete C callback control
flow, task-storage binding, and safe-release testimony remain trusted.

The experiment maps two vCPUs to two synthetic LLC groups; it does not claim
that they are two physical LLCs. C is the complete coverage configuration for
these groups, not an implementation of arbitrary CPU masks or a global
multi-partition transaction. Conservative Detached accounting may delay drain
retirement. Liveness, fairness, and correctness of all paths on the deployment
kernel remain future integration work.
