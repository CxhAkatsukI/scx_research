#!/usr/bin/env python3
"""Check mutations of the real policy, without editing its source file."""

from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parent.parent
source = (root / "src/sched/bpf/main.rs").read_text()
cases = (
    ("zero_weight", "SCX_SLICE_DFL, p.weight()", "SCX_SLICE_DFL, 0",
     "precondition not satisfied"),
    ("positive_init", "scx::create_dsq(SHARED_DSQ, -1)", "1",
     "postcondition not satisfied"),
)

for name, before, after, diagnostic in cases:
    if source.count(before) != 1:
        raise SystemExit(f"{name}: expected exactly one mutation site")
    out = root / "build" / f"reject_{name}"
    out.mkdir(parents=True, exist_ok=True)
    mutant = out / "main.rs"
    mutant.write_text(source.replace(before, after))
    # Remove the stamp so a previous mutation can never bypass verification.
    (out / "verify.stamp").unlink(missing_ok=True)
    result = subprocess.run(
        ["make", "--no-print-directory", "-C", str(root / "src/sched"),
         "verify", f"PROG=reject_{name}", f"SRC={mutant}"],
        text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    (out / "make.log").write_text(result.stdout)
    log = (out / "verify.log").read_text() if (out / "verify.log").exists() else ""
    # A missing dependency or Rust parse failure is not a successful negative test.
    if (result.returncode == 0 or diagnostic not in log
            or not re.search(r"verification results:: \d+ verified, [1-9]\d* errors", log)):
        raise SystemExit(f"{name}: unexpected result; inspect {out / 'make.log'}")
    print(f"PASS: {name} rejected by its {diagnostic.split()[0]} ({out / 'verify.log'})")
