#!/usr/bin/env python3
"""Run real callback bodies against an explicitly delayed mock kernel."""
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
out = root / "build/callback-tests"
out.mkdir(parents=True, exist_ok=True)

def run(args):
    subprocess.run([str(arg) for arg in args], check=True)

run(["rustc", "--edition=2021", "--crate-type=lib", "--crate-name=p1_core",
     "-C", "opt-level=3", "-C", "panic=abort", "--emit=obj",
     root / "bpf/core.rs", "-o", out / "core.o"])
run([os.environ.get("CLANG", "clang-23"), "-O2", "-g", "-Wall", "-Werror",
     "-fsanitize=address,undefined", root / "tests/callbacks.c", out / "core.o",
     "-o", out / "callbacks"])
run([out / "callbacks"])
