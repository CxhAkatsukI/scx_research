# Attach Test

This test must run on a Linux host with `CONFIG_SCHED_CLASS_EXT=y`. It has not been run on the current WSL host.

Install `bpftool`, libbpf headers, a nightly Cargo, `rust-src` for the selected
BPF Rust toolchain, and matching LLVM versions for Rust, clang, and llvm-link.
Then build the BPF object:

```sh
cd sched_ext_verus
./bpf/build.sh
```

The script generates `target/bpf/vmlinux.h` from the running kernel. Set
`VMLINUX_H`, `RUSTC_BPF_TOOLCHAIN`, or the LLVM tool variables when the local
tool names differ. The build rejects mismatched LLVM major versions.

Before registering the scheduler, keep a root shell open for recovery. Then run:

```sh
sudo mkdir -p /sys/fs/bpf/rust_minimal
sudo bpftool -d struct_ops register \
  target/bpf/rust_minimal.bpf.o /sys/fs/bpf/rust_minimal
cat /sys/kernel/sched_ext/state
cat /sys/kernel/sched_ext/*/ops
```

Run a short `SCHED_EXT` workload in another shell and then unregister. On a
system where the scheduler switches all normal tasks, plain `yes` is enough.

```sh
chrt -e 0 yes > /dev/null &
workload_pid=$!
sleep 2
kill "$workload_pid"
sudo rm /sys/fs/bpf/rust_minimal/minimal_ops
sudo rmdir /sys/fs/bpf/rust_minimal
```

If registration fails, keep the complete `bpftool -d` verifier output. It determines whether the mismatch is in callback ABI, kfunc signatures, or the BPF verifier.
