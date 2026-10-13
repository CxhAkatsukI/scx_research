#!/usr/bin/env python3
"""Build a guest-only loader and bounded workload; never execute it here."""
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
out = root / "build/bpf"
with (out / "p1.skel.h").open("w") as header:
    subprocess.run([os.environ.get("BPFTOOL", "bpftool"), "gen", "skeleton",
                    str(out / "problem1.bpf.o"), "name", "p1"], stdout=header, check=True)
subprocess.run([os.environ.get("CC", "cc"), "-O2", "-g", "-Wall", "-Wextra", "-Werror",
                "-I", str(out), str(root / "tests/vm_smoke.c"), "-lbpf",
                "-o", str(out / "vm-smoke")], check=True)
print(f"Built QEMU-only smoke test: {out / 'vm-smoke'}")
