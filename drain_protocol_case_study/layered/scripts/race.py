#!/usr/bin/env python3
"""Instrument the pinned checkout and check causal traces from the VM."""

from collections import Counter, defaultdict
from pathlib import Path
import sys


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
                  '#include "race.bpf.h"\n\nstatic inline bool refresh_layer_cpuc(')
    old = """\t\tlayer_llc_drain_enable(layer, llc_id);
\t\tlayer_kick_idle_node_cpu(layer, llc_node_id(llc_id));"""
    bpf = replace(bpf, old, """\t\tlayer_llc_drain_enable(layer, llc_id);
\t\tif (layer_id == 0)
\t\t\tp1_requested(p->pid, taskc->dsq_id);
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

        // The skeleton attaches and owns the tracing links.
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
    rust = replace(rust, "        let _ = self.struct_ops.take();\n        uei_report!",
                   """        let _ = self.struct_ops.take();
        self.p1_events.consume()?;
        eprintln!("P1_LOST {}", self.skel.maps.bss_data.as_ref().unwrap().p1_lost);
        uei_report!""")
    write_if_changed(dest / path / "bpf/main.bpf.c", bpf)
    write_if_changed(dest / path / "bpf/race.bpf.h", Path(__file__).with_name("race.bpf.h").read_text())
    write_if_changed(dest / path / "main.rs", rust)


def report(path):
    events = defaultdict(list)
    ready, lost = 0, []
    for line in path.read_text(errors="replace").splitlines():
        if line.startswith("P1 "):
            kind, *fields = map(int, line.split()[1:])
            if kind not in (1, 2, 3) or len(fields) != 9:
                raise ValueError(f"invalid trace record: {line}")
            events[kind].append(fields)
        elif line == "P1_PROBES_ATTACHED":
            ready += 1
        elif line.startswith("P1_LOST "):
            lost.append(int(line.split()[1]))

    requests = {(dsq, pid, issued) for dsq, pid, issued, *_ in events[1]}
    clears = defaultdict(list)
    for dsq, _, begin, end, _, q, *_ in events[2]:
        if q == 0 and begin < end:
            clears[dsq].append((begin, end))
    same_dsq = set(clears) & {event[0] for event in events[3]}
    matches = []
    for dsq, pid, enter, end, issued, q, c, d, on_dsq in events[3]:
        if not (q > 0 and c == d == 0 and on_dsq == 1 and enter < end):
            continue
        if (dsq, pid, issued) not in requests:
            continue
        for clear, empty in clears.get(dsq, ()):
            if issued < clear < empty < enter:
                matches.append((dsq, pid, issued, clear, empty, enter, end, q))
                break

    print(f"Requests: {len(events[1])}; drain rechecks: {len(events[2])}; "
          f"empty rechecks: {sum(map(len, clears.values()))}; correlated kernel landings: {len(events[3])}")
    print(f"DSQs with both empty rechecks and landings: {sorted(same_dsq)}; "
          f"probe sessions: {ready}; dropped records by session: {lost}")
    print(f"Landing observations (C,D): {dict(sorted(Counter((e[6], e[7]) for e in events[3]).items()))}")
    if ready != 1 or lost != [0] or not same_dsq:
        print("Coverage: incomplete; a negative result is inconclusive.")
    if ready == 1 and len(lost) == 1 and matches:
        print(f"MATCHING RACE TRACE: {len(matches)} landing(s)")
        dsq, pid, issue, clear, empty, enter, end, q = matches[0]
        print(f"layer=0 llc={dsq} pid={pid} Q={q} C=0 D=0")
        print(f"ns: request+enable={issue} before_clear={clear} empty_recheck={empty} "
              f"kernel_entry={enter} landing_observation={end}")
        print("C/D and Q were sampled, not read as one atomic snapshot.")
    elif ready == 1 and lost == [0] and same_dsq:
        print("NO MATCH in this run; this does not rule out the race.")
    else:
        print("INCONCLUSIVE: missing probe, shutdown, or workload coverage evidence.")


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "instrument":
        instrument(Path(sys.argv[2]), Path(sys.argv[3]))
    elif len(sys.argv) == 3 and sys.argv[1] == "report":
        report(Path(sys.argv[2]))
    else:
        sys.exit("usage: race.py instrument SRC DEST | race.py report LOG")
