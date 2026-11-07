#!/usr/bin/env python3
"""Negative controls must fail proof obligations, not parsing or compilation."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
VERUS = Path(sys.argv[1]).resolve()
CASES = (
    ("read coverage before requesting insertion", "policy.rs",
     "api.dsq_insert(log);\n    if !api.nr_llc_cpus_nonzero(log) {",
     "if !api.nr_llc_cpus_nonzero(log) {\n        api.dsq_insert(log);"),
    ("omit post-consume repair", "policy.rs",
     "if disabled && api.dsq_nr_queued(log) > 0 {", "if false {"),
    ("silently make deferred landing immediate", "model.rs",
     "q: if deferred { s.q + 1 } else { s.q }, landed: true,",
     "q: s.q, landed: true,"),
    ("permit stale-zero observations in immediate safety theorem", "model.rs",
     "Op::Queued(n) => n == s.q,", "Op::Queued(n) => n == s.q || n == 0,"),
)
for name, filename, before, after in CASES:
    with tempfile.TemporaryDirectory(prefix="layered-mutant-") as tmp:
        dest = Path(tmp)
        shutil.copytree(ROOT / "src", dest / "src")
        file = dest / "src" / filename
        text = file.read_text()
        if text.count(before) != 1:
            raise SystemExit(f"ambiguous mutation: {name}")
        file.write_text(text.replace(before, after))
        result = subprocess.run([str(VERUS), "--crate-type=lib", "src/lib.rs"], cwd=dest,
                                capture_output=True, text=True, timeout=60)
        output = result.stdout + result.stderr
        if result.returncode == 0 or "verification results::" not in output or not any(
                s in output for s in ("precondition not satisfied", "postcondition not satisfied", "assertion failed")):
            raise SystemExit(f"expected proof rejection: {name}\n{output}")
        print(f"rejected: {name}")
