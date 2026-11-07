#!/usr/bin/env python3
"""Fetch exact source blobs and extract unchanged protocol statements."""
import hashlib
import base64
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.request import Request, urlopen
from urllib.error import URLError

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT.parent / "scripts"))
from extract import block_end


def blob_id(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def fetch():
    manifest = json.loads((ROOT / "upstream.json").read_text())
    for item in manifest["files"]:
        dest = ROOT / "dep" / item["name"]
        if dest.exists() and blob_id(dest.read_bytes()) == item["blob"]:
            continue
        data = None
        if item["repo"] == "sched-ext/scx":
            local = ROOT.parent / "dep/scx"
            if local.exists():
                result = subprocess.run(["git", "--no-lazy-fetch", "-C", str(local),
                                         "show", item["blob"]], capture_output=True)
                if result.returncode == 0:
                    data = result.stdout
        else:
            local = ROOT.parents[1] / "problem1_accounting/build/research/ext-v6.16.c"
            if local.exists():
                data = local.read_bytes()
        if data is None:
            url = f"https://raw.githubusercontent.com/{item['repo']}/{item['rev']}/{item['path']}"
            try:
                with urlopen(Request(url, headers={"User-Agent": "layered-protocol-study"}), timeout=15) as r:
                    data = r.read()
            except (URLError, TimeoutError):
                url = f"https://api.github.com/repos/{item['repo']}/git/blobs/{item['blob']}"
                with urlopen(Request(url, headers={"User-Agent": "layered-protocol-study"}), timeout=30) as r:
                    data = base64.b64decode(json.load(r)["content"])
        if blob_id(data) != item["blob"]:
            raise SystemExit(f"source identity mismatch: {dest}")
        dest.parent.mkdir(exist_ok=True)
        dest.write_bytes(data)
    print("checked layered, compat and Linux v6.16 source identities")


def whole(text, name):
    matches = list(re.finditer(r"^static (?:inline )?[^\n]*\b" + name + r"\(", text, re.M))
    if len(matches) != 1:
        raise ValueError(f"ambiguous function {name}")
    start = matches[0].start()
    return text[start:block_end(text, text.index("{", start))]


def extract():
    text = (ROOT / "dep/layered.bpf.c").read_text()
    start = text.index("if (layer->fifo)", text.index("void BPF_STRUCT_OPS(layered_enqueue,"))
    end = block_end(text, text.index("{", text.index("if (!layer->nr_llc_cpus[llc_id])", start)))
    enqueue = text[start:end]
    # Instrument the plain shared-field read, preserving its expression/branch point.
    needle = "layer->nr_llc_cpus[llc_id]"
    if enqueue.count(needle) != 1:
        raise ValueError("coverage instrumentation is ambiguous")
    enqueue = enqueue.replace(needle, "read_coverage(layer, llc_id)")
    drain_fn = whole(text, "try_drain_layer_node_llcs")
    start = drain_fn.index("if (scx_bpf_dsq_nr_queued(dsq_id) <= 1)")
    end = drain_fn.index("if (consumed)", start)
    drain = drain_fn[start:end]
    parts = ['// Generated unchanged statements from the pinned layered source.\n#include "oracle.h"\n']
    parts.extend(whole(text, name) for name in ("layer_llc_drain_enable", "layer_llc_drain_disable"))
    parts.append("void source_enqueue(void) {\n"
                 "struct layer *layer = &shared; struct task_ctx *taskc = &task;\n"
                 "void *p = 0; u32 layer_id = 0, llc_id = 0; u64 enq_flags = 0, vtime = 0;\n"
                 "(void)layer_id;\n" + enqueue + "\n}\n")
    parts.append("bool source_drain(void) {\nstruct layer *layer = &shared;\n"
                 "u32 llc_id = 0; u64 dsq_id = 0; bool disabled = false, consumed;\n"
                 + drain + "\nreturn consumed;\n}\n")
    out = ROOT / "build"
    out.mkdir(exist_ok=True)
    (out / "source.c").write_text("\n".join(parts))
    print("extracted whole drain-bit helpers, enqueue tail, and selected drain branch")


if __name__ == "__main__":
    fetch()
    extract()
