#!/usr/bin/env python3
"""Offline ELF build: actual Rust word bodies + trusted C kernel callbacks."""
import os
import hashlib
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
stage1 = root.parent / "lachesis_step1"
out = root / "build/bpf"
out.mkdir(parents=True, exist_ok=True)
vmlinux = Path(os.environ.get("VMLINUX", "/sys/kernel/btf/vmlinux"))
if not vmlinux.is_file():
    raise SystemExit("Set VMLINUX=/absolute/path/to/target/vmlinux (ELF or raw BTF)")
rustc = os.environ.get("BPF_RUSTC", str(Path.home() / ".rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc"))
deps = Path(os.environ.get("BPF_RUST_DEPS", stage1 / "build/rust-deps"))
target = stage1 / "src/toolchain/bpfel-unknown-none-v4.json"
if not (deps / "libcore.rlib").is_file():
    raise SystemExit("Build lachesis_step1's pinned BPF libcore first; see its README")

def run(args, **kwargs):
    subprocess.run([str(arg) for arg in args], check=True, **kwargs)

btf = vmlinux.read_bytes()
if btf.startswith(b"\x7fELF"):
    run(["llvm-objcopy", "-O", "binary", "--only-section=.BTF", vmlinux, out / "vmlinux.btf"])
    btf = (out / "vmlinux.btf").read_bytes()

with (out / "vmlinux.h").open("w") as header:
    run([os.environ.get("BPFTOOL", "bpftool"), "btf", "dump", "file", vmlinux, "format", "c"], stdout=header)
env = dict(os.environ, RUSTC_BOOTSTRAP="1")
run([rustc, "--edition=2021", "--crate-type=rlib", "--crate-name=p1_core",
     "--target", target, "-Z", "unstable-options", "--sysroot=/dev/null",
     "-L", deps, "-C", "opt-level=3", "-C", "panic=unwind", "--emit=llvm-bc",
     "-o", out / "core.bc", root / "bpf/core.rs"], env=env)
run([os.environ.get("CLANG", "clang-23"), "-target", "bpfel", "-mcpu=v4", "-O2", "-g",
     "-Wall", "-Werror", "-isystem", out, "-emit-llvm", "-c", root / "bpf/callbacks.bpf.c",
     "-o", out / "callbacks.bc"])
run(["llvm-link", out / "core.bc", out / "callbacks.bc", "-o", out / "linked.bc"])
api = "p1_ops,p1_select_cpu,p1_enqueue,p1_dispatch,p1_running,p1_disable,p1_init_task,p1_exit_task,p1_init,p1_config,packed,cpu_llc,tickets,LICENSE"
run(["opt", "--internalize-public-api-list=" + api,
     "-passes=internalize,globaldce,default<O2>", out / "linked.bc", "-o", out / "optimized.bc"])
run(["llc", "-march=bpfel", "-mcpu=v4", "-filetype=obj", out / "optimized.bc", "-o", out / "problem1.bpf.o"])
with (out / "disassembly.txt").open("w") as disassembly:
    run(["llvm-objdump", "-dr", out / "problem1.bpf.o"], stdout=disassembly)
assembly = (out / "disassembly.txt").read_text()
if "cmpxchg" not in assembly:
    raise SystemExit("No cmpxchg in generated object: atomic lowering must be reviewed")
undefined = subprocess.check_output(["llvm-nm", "--undefined-only", str(out / "problem1.bpf.o")], text=True)
if "p1_" in undefined or "panic" in undefined or "rust_" in undefined:
    raise SystemExit("Unexpected unresolved Rust/core symbol:\n" + undefined)
(out / "build-info.txt").write_text(
    f"VMLINUX={vmlinux.resolve()}\n"
    f"VMLINUX_SHA256={hashlib.sha256(vmlinux.read_bytes()).hexdigest()}\n"
    f"BTF_SHA256={hashlib.sha256(btf).hexdigest()}\n"
    f"OBJECT_SHA256={hashlib.sha256((out / 'problem1.bpf.o').read_bytes()).hexdigest()}\n"
    + undefined)
print(f"Built {out / 'problem1.bpf.o'} (offline only; not kernel-verifier or VM evidence)")
