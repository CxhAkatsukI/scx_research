import re
import json
import time
import sys

PIPE_PATH = "/sys/kernel/tracing/trace_pipe"
OUTPUT_FILE = "trace.json"

# use regx to parse kernel log
log_pattern = re.compile(r'^\s*.*?\s+(\d+\.\d+):\s+bpf_trace_printk:\s+\[SCX\] CPU=(\d+) EV=(START|STOP) COMM=(.*)$')

events = []
active_tasks = {} # Record active tasks by CPU, key: cpu, value: {"comm": comm, "start_ts": ts_us}

print(f"Begin probing kernel data, save to {OUTPUT_FILE}")
print("Run workloads on other window")
print("Press Ctrl+C to stop recording and generate JSON...")

try:
    with open(PIPE_PATH, "r") as f:
        for line in f:
            match = log_pattern.match(line)
            if match:
                ts_us = int(float(match.group(1)) * 1_000_000)
                cpu = int(match.group(2))
                event = match.group(3)
                comm = match.group(4).strip()

                if event == "START":
                    active_tasks[cpu] = {"comm": comm, "start_ts": ts_us}
                elif event == "STOP" and cpu in active_tasks:
                    start_ts = active_tasks[cpu]["start_ts"]
                    dur = ts_us - start_ts
                    
                    # Generate Perfetto compatible event
                    events.append({
                        "name": active_tasks[cpu]["comm"],
                        "cat": "sched",
                        "ph": "X",       # X 代表一个拥有持续时间的完整事件
                        "ts": start_ts,
                        "dur": dur,
                        "pid": 0,        # 统一为一个假进程
                        "tid": f"CPU {cpu}", # 以 CPU 作为横轴轨道 (Track)
                    })
                    del active_tasks[cpu]
except KeyboardInterrupt:
    print("\nRecording stopped by user, generating JSON")

# write events to JSON file
with open(OUTPUT_FILE, "w") as f:
    json.dump(events, f, indent=2)

print(f"successfully saved {len(events)} events to {OUTPUT_FILE}")
print("Use Perfetto UI to visualize the trace: https://ui.perfetto.dev/")