#!/usr/bin/env python3
"""Reject semantic bugs in the actual callbacks, CAS loop, and protocol proofs."""
import os
from pathlib import Path
import re
import shutil
import subprocess

root = Path(__file__).resolve().parent.parent
out = root / "build/mutants"
out.mkdir(parents=True, exist_ok=True)
verus = os.environ.get("VERUS", str(root.parent / "lachesis_step1/dep/verus/source/target-verus/release/verus"))
deps = root / "build/host/verus"
imports = []
for name in ("p1_protocol", "lachesis_runtime_trusted", "p1_runtime"):
    imports += ["--extern", f"{name}={deps / ('lib' + name + '.rlib')}",
                "--import", f"{name}={deps / (name + '.vir')}"]

# The scheduler must use the checked primitive wrappers, not forge/reset receipts.
for path in (root / "src/runtime/lib.rs", root / "src/sched/main.rs"):
    if re.search(r"Log::new|\.record\(|\*log\s*=", path.read_text()):
        raise SystemExit(f"Unexpected direct monitor manipulation: {path}")

cases = [
    ("enqueue_without_count", "sched/main.rs",
     "if self.shared.update(REGISTER, false, log)", "if true", "precondition not satisfied"),
    ("enqueue_without_ticket", "sched/main.rs",
     "t.set_owned(true, log);", "", "precondition not satisfied"),
    ("release_without_count", "runtime/lib.rs",
     "if !self.update(RELEASE, false, log) { return false; }", "", "precondition not satisfied"),
    ("register_without_drain", "protocol/word.rs",
     "(w + 4) | if w & 1 == 0 { 2 } else { 0 }", "w + 4", "postcondition not satisfied"),
    ("retire_nonempty", "protocol/word.rs",
     "let r = if w >> 2 == 0 { w & !2u64 } else { w };", "let r = w & !2u64;", "postcondition not satisfied"),
    ("unaccounted_reserved", "protocol/model.rs",
     "p == Phase::Off || p == Phase::Releasing", "p == Phase::Off || p == Phase::Releasing || p == Phase::Reserved", "postcondition not satisfied"),
    ("overlapping_task_callbacks", "protocol/composition.rs",
     "&& (forall|j: int| #![auto] 0 <= j < s.calls.len() && s.calls[j].active && task >= 0\n                ==> s.calls[j].task != task)", "", "postcondition not satisfied"),
]

for name, relative, old, new, diagnostic in cases:
    dest = out / name
    shutil.copytree(root / "src", dest, dirs_exist_ok=True)
    path = dest / relative
    text = path.read_text()
    if text.count(old) != 1:
        raise SystemExit(f"Mutation {name} no longer matches exactly once")
    path.write_text(text.replace(old, new))
    if relative.startswith("protocol/"):
        args = ["--no-cheating", "--crate-name", "p1_protocol", str(dest / "protocol/lib.rs")]
    else:
        args = imports + ["-L", f"dependency={deps}", "-L", f"dependency={root / 'build/host/rust-deps'}",
                          "-L", f"dependency={root / 'build/host/btf-macros/release/deps'}", str(path)]
    result = subprocess.run([verus, "--crate-type=lib", *args], text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=120)
    (out / f"{name}.log").write_text(result.stdout)
    if result.returncode == 0 or diagnostic not in result.stdout:
        print(result.stdout[-5000:])
        raise SystemExit(f"Expected verification rejection ({diagnostic}): {name}")
    print(f"OK: rejected {name}")
