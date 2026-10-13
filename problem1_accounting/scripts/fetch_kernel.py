#!/usr/bin/env python3
"""Fetch the research baseline and check Git blob identities, not a model."""
import base64
import hashlib
import json
from pathlib import Path
from urllib.request import Request, urlopen

root = Path(__file__).resolve().parent.parent
out = root / "build/research"
out.mkdir(parents=True, exist_ok=True)
files = (
    ("kernel/sched/ext.c", "ext-v6.16.c", "7dd5cbcb7a069deea2789a1bad78d5f21770a6d9"),
    ("kernel/sched/core.c", "core-v6.16.c", "81c6df746df17b88b1d5bc5a61c5c9d98e07084f"),
    ("kernel/sched/syscalls.c", "syscalls-v6.16.c", "547c1f05b667ef1a381989bb984015c7c3838344"),
)

def blob_id(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()

for source, name, expected in files:
    path = out / name
    if path.is_file() and blob_id(path.read_bytes()) == expected:
        print(f"checked cached {name}: {expected}")
        continue
    request = Request(f"https://api.github.com/repos/torvalds/linux/contents/{source}?ref=v6.16",
                      headers={"Accept": "application/vnd.github+json", "User-Agent": "problem1-accounting-research"})
    with urlopen(request, timeout=45) as response:
        result = json.load(response)
    data = base64.b64decode(result["content"])
    if result["sha"] != expected or blob_id(data) != expected:
        raise SystemExit(f"Kernel source identity mismatch: {source}")
    path.write_bytes(data)
    print(f"fetched {name}: {expected}")
