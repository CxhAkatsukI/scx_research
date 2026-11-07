#!/usr/bin/env python3
"""Fetch pinned evidence, preferring the local history; verify Git blob hashes."""
import base64
import hashlib
import json
from pathlib import Path
import subprocess
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent


def blob_id(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def main():
    manifest = json.loads((ROOT / "upstream.json").read_text())
    for item in manifest["files"]:
        dest = ROOT / "dep/evidence" / item["version"] / Path(item["path"]).name
        expected = item["blob"]
        if dest.is_file() and blob_id(dest.read_bytes()) == expected:
            print(f"checked {item['version']}/{dest.name}: {expected}")
            continue
        local = ROOT / "dep/scx"
        data = None
        if (local / ".git").is_dir():
            result = subprocess.run(["git", "-C", str(local), "show", expected],
                                    capture_output=True, timeout=60)
            if result.returncode == 0:
                data = result.stdout
        if data is None:
            request = Request(f"https://api.github.com/repos/sched-ext/scx/git/blobs/{expected}",
                              headers={"User-Agent": "drain-protocol-case-study"})
            with urlopen(request, timeout=60) as response:
                data = base64.b64decode(json.load(response)["content"])
        if blob_id(data) != expected:
            raise SystemExit(f"source identity mismatch: {dest}")
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        print(f"fetched {item['version']}/{dest.name}: {expected}")


if __name__ == "__main__":
    main()
