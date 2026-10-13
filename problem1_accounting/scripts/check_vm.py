#!/usr/bin/env python3
"""Boot a bounded VM for verifier or partial-switch smoke testing."""
import argparse
import hashlib
import os
from pathlib import Path
import shlex
import signal
import subprocess

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument("--smoke", action="store_true")
args = parser.parse_args()
out = root / "build/vm"
out.mkdir(parents=True, exist_ok=True)
marker = out / ("smoke-passed" if args.smoke else "passed")
marker.unlink(missing_ok=True)
vng = Path(os.environ.get("VNG", root.parent / "lachesis_step1/build/vm-tools/bin/vng"))
kernel = Path(os.environ.get("KERNEL", "/boot/vmlinuz-linux"))
obj = root / "build/bpf/problem1.bpf.o"
if not vng.is_file() or not kernel.is_file() or not obj.is_file():
    raise SystemExit("Need VNG, KERNEL and a built BPF object (make bpf)")
info_path = root / "build/bpf/build-info.txt"
info = dict(line.split("=", 1) for line in info_path.read_text().splitlines() if "=" in line)
if hashlib.sha256(obj.read_bytes()).hexdigest() != info.get("OBJECT_SHA256"):
    raise SystemExit("BPF object does not match its build record; run make bpf")
script = "vm-smoke.sh" if args.smoke else "vm-verifier.sh"
artifact = root / "build/bpf/vm-smoke" if args.smoke else obj
if not artifact.is_file():
    raise SystemExit("Build the VM smoke loader first")
command = shlex.join(["bash", str(root / "scripts" / script), str(artifact), str(out),
                      info["BTF_SHA256"]])
vm_args = [str(vng), "--run", str(kernel), "--user", "root", "--cpus", "4",
        "--memory", "2G", "--disable-microvm", "--rwdir", str(out), "--", command]
env = dict(os.environ)
env["PATH"] = str(vng.parent) + os.pathsep + env.get("PATH", "")
run_log = out / ("smoke-run.log" if args.smoke else "run.log")
with run_log.open("w") as log:
    p = subprocess.Popen(vm_args, stdout=log, stderr=subprocess.STDOUT, start_new_session=True, env=env)
    try:
        rc = p.wait(timeout=60)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGTERM)
        try:
            p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            p.wait()
        raise SystemExit(f"VM timed out; see {run_log}")
if rc or not marker.is_file():
    print(run_log.read_text()[-10000:])
    raise SystemExit(f"VM check failed; see {run_log}")
print(run_log.read_text()[-2000:])
print(f"QEMU check passed; evidence: {out}")
