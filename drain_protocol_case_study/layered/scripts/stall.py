#!/usr/bin/env python3
"""Instrument scx_layered and monitor target progress after a matching race."""

import json
import os
from pathlib import Path
import sys
import time


def replace(source, old, new):
    if source.count(old) != 1:
        raise ValueError(f"expected exactly one upstream anchor: {old!r}")
    return source.replace(old, new, 1)


def write_if_changed(path, text):
    if not path.exists() or path.read_text() != text:
        path.write_text(text)


def instrument(src, dest):
    path = Path("scheds/rust/scx_layered/src")
    bpf = (src / path / "bpf/main.bpf.c").read_text()
    bpf = replace(bpf, "static inline bool refresh_layer_cpuc(",
                  '#include "stall.bpf.h"\n\nstatic inline bool refresh_layer_cpuc(')
    old = """\t\tlayer_llc_drain_enable(layer, llc_id);
\t\tlayer_kick_idle_node_cpu(layer, llc_node_id(llc_id));"""
    bpf = replace(bpf, old, """\t\tlayer_llc_drain_enable(layer, llc_id);
\t\tif (layer_id == 0)
\t\t\tp1_requested(p, taskc->dsq_id);
\t\tlayer_kick_idle_node_cpu(layer, llc_node_id(llc_id));""")
    old = "\t\tif (scx_bpf_dsq_nr_queued(dsq_id) <= 1) {\n\t\t\tlayer_llc_drain_disable(layer, llc_id);"
    bpf = replace(bpf, old, """\t\tu64 p1_before_clear = 0;
\t\tif (scx_bpf_dsq_nr_queued(dsq_id) <= 1) {
\t\t\tif (layer->id == 0)
\t\t\t\tp1_before_clear = bpf_ktime_get_ns();
\t\t\tlayer_llc_drain_disable(layer, llc_id);""")
    bpf = replace(bpf, """\t\tif (disabled && scx_bpf_dsq_nr_queued(dsq_id))
\t\t\tlayer_llc_drain_enable(layer, llc_id);""", """\t\tif (disabled) {
\t\t\tu32 queued = scx_bpf_dsq_nr_queued(dsq_id);
\t\t\tif (queued)
\t\t\t\tlayer_llc_drain_enable(layer, llc_id);
\t\t\tif (p1_before_clear)
\t\t\t\tp1_rechecked(dsq_id, p1_before_clear, queued);
\t\t}""")
    bpf = replace(bpf, """void BPF_STRUCT_OPS(layered_running, struct task_struct *p)
{""", """void BPF_STRUCT_OPS(layered_running, struct task_struct *p)
{
\tp1_running(p);""")

    rust = (src / path / "main.rs").read_text()
    rust = replace(rust, "    struct_ops: Option<libbpf_rs::Link>,",
                   """    struct_ops: Option<libbpf_rs::Link>,
    p1_events: libbpf_rs::RingBuffer<'static>,""")
    rust = replace(rust, "        // Attach.\n        let struct_ops = scx_ops_attach!(skel, layered)?;",
                   """        let mut builder = libbpf_rs::RingBufferBuilder::new();
        builder.add(&skel.maps.p1_events, |data| {
            if data.len() != 80 {
                return -1;
            }
            let fields = data.chunks_exact(8)
                .map(|b| u64::from_ne_bytes(b.try_into().unwrap()).to_string())
                .collect::<Vec<_>>().join(" ");
            eprintln!("P1 {fields}");
            0
        })?;
        let p1_events = builder.build()?;

        let struct_ops = scx_ops_attach!(skel, layered)?;
        if skel.links.p1_land_enter.is_none() || skel.links.p1_land_exit.is_none() {
            anyhow::bail!("dispatch_enqueue tracing links were not attached");
        }
        eprintln!("P1_PROBES_ATTACHED");""")
    rust = replace(rust, "            struct_ops: Some(struct_ops),",
                   """            struct_ops: Some(struct_ops),
            p1_events,""")
    rust = replace(rust, "        while !shutdown.load(Ordering::Relaxed) && !uei_exited!(&self.skel, uei) {",
                   """        while !shutdown.load(Ordering::Relaxed) && !uei_exited!(&self.skel, uei) {
            self.p1_events.consume()?;""")
    rust = replace(rust, "        self.refresh_cpumasks()?;\n        self.refresh_xnuma();",
                   """        if self.skel.maps.bss_data.as_ref().unwrap().p1_freeze == 0 {
            self.refresh_cpumasks()?;
        }
        self.refresh_xnuma();""")
    rust = replace(rust, "        let _ = self.struct_ops.take();\n        uei_report!",
                   """        let _ = self.struct_ops.take();
        self.p1_events.consume()?;
        eprintln!("P1_LOST {}", self.skel.maps.bss_data.as_ref().unwrap().p1_lost);
        uei_report!""")
    write_if_changed(dest / path / "bpf/main.bpf.c", bpf)
    write_if_changed(dest / path / "bpf/stall.bpf.h", Path(__file__).with_name("stall.bpf.h").read_text())
    write_if_changed(dest / path / "main.rs", rust)


def proc_sample(pid):
    try:
        comm = Path(f"/proc/{pid}/comm").read_text().strip()
        stat = Path(f"/proc/{pid}/stat").read_text()
        rest = stat[stat.rfind(")") + 2:].split()
        return {"comm": comm, "state": rest[0], "ticks": int(rest[11]) + int(rest[12])}
    except (FileNotFoundError, ProcessLookupError, IndexError, ValueError):
        return None


def monitor(trace_path, output_path, duration, freeze_path):
    deadline = time.monotonic() + duration
    pending = {}
    completed = []
    ignored = 0
    ready = 0
    lost = []
    position = 0

    while time.monotonic() < deadline:
        try:
            with trace_path.open(errors="replace") as trace:
                trace.seek(position)
                lines = trace.readlines()
                position = trace.tell()
        except FileNotFoundError:
            lines = []
        for line in lines:
            line = line.strip()
            if line == "P1_PROBES_ATTACHED":
                ready += 1
            elif line.startswith("P1_LOST "):
                lost.append(int(line.split()[1]))
            elif not line.startswith("P1 "):
                continue
            else:
                fields = list(map(int, line.split()[1:]))
                kind = fields[0]
                if kind == 4 and len(fields) == 10:
                    _, dsq, pid, enter, landed, issued, q, c, d, on_dsq = fields
                    sample = proc_sample(pid)
                    if not sample or sample["comm"] != "stall-target":
                        ignored += 1
                        continue
                    if any(item["pid"] == pid and item["landed"] == landed
                           for item in completed) or (pid, landed) in pending:
                        continue
                    item = {
                        "pid": pid, "dsq": dsq, "issued": issued,
                        "kernel_entry": enter, "landed": landed, "q": q,
                        "detected_monotonic": time.monotonic(),
                        "first_ticks": sample["ticks"], "last_ticks": sample["ticks"],
                        "states": sorted({sample["state"]}), "samples": 1,
                        "rescue_delay_ms": None, "pending_seconds": 0.0,
                    }
                    pending[(pid, landed)] = item
                    Path(freeze_path).write_text(f"{pid}\n")
                    stopped = []
                    for proc in Path("/proc").iterdir():
                        if not proc.name.isdigit() or int(proc.name) == pid:
                            continue
                        try:
                            if (proc / "comm").read_text().strip() == "stall-target":
                                os.kill(int(proc.name), 15)
                                stopped.append(int(proc.name))
                        except (FileNotFoundError, ProcessLookupError, PermissionError):
                            pass
                    print(f"MATCH target pid={pid} dsq={dsq} Q={q} C={c} D={d}; "
                          f"load/config frozen; other targets stopped: {stopped}", flush=True)
                elif kind == 5 and len(fields) == 10:
                    _, dsq, pid, landed, running, issued, *_ = fields
                    item = pending.pop((pid, landed), None)
                    if item is None:
                        continue
                    item["rescue_delay_ms"] = (running - landed) / 1e6
                    item["pending_seconds"] = time.monotonic() - item["detected_monotonic"]
                    completed.append(item)
                    print(f"RESCUE target pid={pid} delay={item['rescue_delay_ms']:.3f} ms", flush=True)

        now = time.monotonic()
        for item in pending.values():
            item["pending_seconds"] = now - item["detected_monotonic"]
            sample = proc_sample(item["pid"])
            if sample:
                item["last_ticks"] = sample["ticks"]
                item["states"] = sorted(set(item["states"]) | {sample["state"]})
                item["samples"] += 1
        if not lines:
            time.sleep(0.05)
        else:
            time.sleep(0.01)

    completed.extend(pending.values())
    for item in completed:
        item["cpu_ticks_observed"] = item["last_ticks"] - item["first_ticks"]
        item.pop("detected_monotonic", None)
    result = {
        "probe_sessions": ready,
        "dropped_records_by_session": lost,
        "ignored_non_target_matches": ignored,
        "target_matches": completed,
        "unrescued_over_5s": [item["pid"] for item in completed
                              if item["rescue_delay_ms"] is None and item["pending_seconds"] >= 5],
        "unrescued_over_5s_without_cpu_progress": [item["pid"] for item in completed
                              if item["rescue_delay_ms"] is None and item["pending_seconds"] >= 5
                              and item["cpu_ticks_observed"] == 0],
    }
    output_path.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2), flush=True)


def report(trace_path):
    matches = {}
    rescues = []
    lost = []
    for line in trace_path.read_text(errors="replace").splitlines():
        if line.startswith("P1_LOST "):
            lost.append(int(line.split()[1]))
        if not line.startswith("P1 "):
            continue
        fields = list(map(int, line.split()[1:]))
        if fields[0] == 4 and len(fields) == 10:
            matches[(fields[2], fields[4])] = fields
        elif fields[0] == 5 and len(fields) == 10:
            rescues.append(fields)
    rescued_keys = {(event[2], event[3]) for event in rescues}
    print(f"Causal matches: {len(matches)}; rescues: {len(rescues)}; dropped records: {lost}")
    for event in rescues:
        print(f"Rescue: pid={event[2]} delay={(event[4] - event[3]) / 1e6:.3f} ms")
    unresolved = [key for key in matches if key not in rescued_keys]
    print(f"Unresolved at loader exit (detach may rescue them): {unresolved}")


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "instrument":
        instrument(Path(sys.argv[2]), Path(sys.argv[3]))
    elif len(sys.argv) == 6 and sys.argv[1] == "monitor":
        monitor(Path(sys.argv[2]), Path(sys.argv[3]), float(sys.argv[4]), sys.argv[5])
    elif len(sys.argv) == 3 and sys.argv[1] == "report":
        report(Path(sys.argv[2]))
    else:
        sys.exit("usage: stall.py instrument SRC DEST | stall.py monitor TRACE OUT SECONDS FREEZE | stall.py report LOG")
