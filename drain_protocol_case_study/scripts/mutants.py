#!/usr/bin/env python3
"""Check that source-order and shared-observation mistakes break verification."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
VERUS = Path(sys.argv[1]).resolve()
mutants = (
    ("read coverage before counting", "policy.rs",
     "s.nr_queued.fetch_add(log);\n    if s.llcs_with_cpus.load(log) { return; }",
     "if s.llcs_with_cpus.load(log) { return; }\n    s.nr_queued.fetch_add(log);"),
    ("omit repair recheck", "policy.rs",
     "if fixed && s.nr_queued.load(log) != 0 {", "if false {"),
    ("omit repair kick", "policy.rs",
     "s.llcs_to_drain.fetch_or(log);\n        kick_subcell_idle_cpu(log);",
     "s.llcs_to_drain.fetch_or(log);"),
    ("allow stale zero in every read", "model.rs",
     "Op::Pending(n) => n == s.n,", "Op::Pending(n) => n == s.n || n == 0,"),
)

for name, filename, before, after in mutants:
    with tempfile.TemporaryDirectory(prefix="drain-case-mutant-") as tmp:
        dest = Path(tmp)
        shutil.copytree(ROOT / "src", dest / "src")
        file = dest / "src" / filename
        text = file.read_text()
        if text.count(before) != 1:
            raise SystemExit(f"ambiguous mutation: {name}")
        file.write_text(text.replace(before, after))
        result = subprocess.run([str(VERUS), "--crate-type=lib", "src/lib.rs"],
                                cwd=dest, capture_output=True, text=True, timeout=60)
        output = result.stdout + result.stderr
        if result.returncode == 0 or "verification results::" not in output:
            raise SystemExit(f"mutant not rejected by proof obligations: {name}\n{output}")
        if "precondition not satisfied" not in output and "postcondition not satisfied" not in output \
                and "assertion failed" not in output:
            raise SystemExit(f"unexpected failure, not a proof rejection: {name}\n{output}")
        print(f"rejected: {name}")
