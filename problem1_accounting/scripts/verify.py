#!/usr/bin/env python3
"""Verify actual word-operation bodies; do not maintain a second algorithm."""
import os
import argparse
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parent.parent
verus = Path(os.environ.get("VERUS", root.parent / "lachesis_step1/dep/verus/source/target-verus/release/verus"))
out = root / "build/verify"
out.mkdir(parents=True, exist_ok=True)
parser = argparse.ArgumentParser()
parser.add_argument("--mutants", action="store_true", help="also require broken executable bodies to fail proof")
args = parser.parse_args()
source = (root / "src/word.rs").read_text()

def wrap(body):
    # Only activate explicit annotations and name return values for contracts.
    body = re.sub(r"^\s*//% ?", "", body, flags=re.M)
    body = re.sub(r"-> u64\n", "-> (r: u64)\n", body)
    return "use vstd::prelude::*;\nverus! {\n" + (root / "verify/spec.rs").read_text() + body + "\n}\n"

def verify(name, target, broken=False):
    p = subprocess.run([str(verus), "--no-cheating", "--crate-type=lib", str(target)],
                       text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (out / f"{name}.log").write_text(p.stdout)
    result = re.search(r"verification results:: (\d+) verified, (\d+) errors", p.stdout)
    if broken:
        # Syntax/type/tool failures do not count as rejecting a bad algorithm.
        if p.returncode == 0 or not result or int(result[2]) == 0:
            raise SystemExit(f"Mutant {name} was not rejected by verification:\n{p.stdout}")
        print(f"rejected {name}: {result[2]} proof error(s)")
    else:
        print(p.stdout, end="")
        if p.returncode or not result or int(result[1]) == 0 or int(result[2]) != 0:
            raise SystemExit(f"No successful nonempty proof run for {name}")

wrapped = wrap(source)
path = out / "protocol.rs"
lifecycle = (root / "verify/lifecycle.rs").read_text()
# The standalone lifecycle crate has exactly one outer verus! block.
inner = lifecycle.split("verus! {\n", 1)[1].rsplit("}", 1)[0]
combined = wrapped.rsplit("}", 1)[0] + inner + (root / "verify/bridge.rs").read_text() + "\n}\n"
path.write_text(combined)
verify("protocol", path)

if args.mutants:
    changes = [
        ("register_without_drain", "r | (4u64 << llc)", "r"),
        ("publish_overwrites_drains", "(w & !3u64)", "(w & !15u64)"),
        ("release_wrong_count", "let r = w - (1u64 << shift);", "let r = w - 1u64;"),
        ("retire_ignores_pending", "if (w >> shift) & 65535 == 0 {", "if true {"),
    ]
    for name, old, new in changes:
        if source.count(old) != 1:
            raise SystemExit(f"Mutation no longer uniquely matches: {name}")
        target = out / f"{name}.rs"
        target.write_text(wrap(source.replace(old, new)))
        verify(name, target, broken=True)
