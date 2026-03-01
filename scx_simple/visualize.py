import re
import json
import sys

PIPE_PATH = "/sys/kernel/tracing/trace_pipe"
OUTPUT_FILE = "trace.json"

# Extracted from the BPF code's printk statements for parsing
log_pattern = re.compile(r'^\s*.*?\s+(\d+\.\d+):\s+bpf_trace_printk:\s+\[SCX\]\s+(.*)$')

events = []
active_tasks = {}  # Track CPU running state: {cpu_id: {"comm": comm, "pid": pid, "start_ts": ts_us}}
waiting_tasks = {} # Track tasks waken but not running: {pid: {"comm": comm, "wake_ts": ts_us}}

# Parse the payload string into a dictionary
def parse_payload(payload_str):
    parts = payload_str.strip().split()
    data = {}
    for part in parts:
        if '=' in part:
            k, v = part.split('=', 1)
            data[k] = v
    return data

print(f"Begin probing kernel data, save to {OUTPUT_FILE}")
print("Run workloads on other window")
print("Press Ctrl+C to stop recording and generate JSON...")

try:
    with open(PIPE_PATH, "r") as f:
        for line in f:
            match = log_pattern.match(line)
            if not match:
                continue
                
            ts_us = int(float(match.group(1)) * 1_000_000)
            payload_str = match.group(2)
            data = parse_payload(payload_str)
            
            event = data.get("EV")
            comm = data.get("COMM", "unknown")
            pid = data.get("PID", "-1")

            # Task wake up
            if event == "ENQUEUE":
                waiting_tasks[pid] = {
                    "comm": comm,
                    "wake_ts": ts_us,
                    "target_cpu": data.get("TARGET_CPU", "unknown")
                }

            # Task starts to run
            elif event == "START":
                cpu = data.get("CPU")
                
                # If this task was in waiting state, calculate wait duration and emit a WAIT event
                if pid in waiting_tasks:
                    wait_dur = ts_us - waiting_tasks[pid]["wake_ts"]
                    if wait_dur > 0:
                        events.append({
                            "name": f"WAIT: {comm} [PID:{pid}]",
                            "cat": "sched",
                            "ph": "X",
                            "ts": waiting_tasks[pid]["wake_ts"],
                            "dur": wait_dur,
                            "pid": 0,
                            "tid": f"CPU {cpu}" 
                        })
                    del waiting_tasks[pid] # delete from waiting state
                
                # Mark this task as running on the CPU
                active_tasks[cpu] = {"comm": comm, "pid": pid, "start_ts": ts_us}

            # Stopping events (either going to sleep or being preempted)
            elif event in ["SLEEP", "PREEMPT", "STOP"]:
                cpu = data.get("CPU")
                if cpu in active_tasks and active_tasks[cpu]["pid"] == pid:
                    start_ts = active_tasks[cpu]["start_ts"]
                    dur = ts_us - start_ts
                    
                    # If the event is SLEEP, we can mark the task as sleeping;
                    # if it's PREEMPT or STOP, we can just mark it as stopped.
                    status_suffix = " (Zzz)" if event == "SLEEP" else ""
                    events.append({
                        "name": f"{active_tasks[cpu]['comm']} [PID:{pid}]{status_suffix}",
                        "cat": "sched",
                        "ph": "X",
                        "ts": start_ts,
                        "dur": dur,
                        "pid": 0,
                        "tid": f"CPU {cpu}",
                    })
                    del active_tasks[cpu]
            
            elif event == "ASSERT":
                cpu = data.get("CPU")
                events.append({
                    "name": "PRIORITY INVERSION",
                    "cat": "sched",
                    "ph": "i",
                    "s": "t",
                    "ts": ts_us,
                    "pid": 0,
                    "tid": f"CPU {cpu}"
                })

except KeyboardInterrupt:
    print("\nRecording stopped by user, generating JSON")

# write events to JSON file
with open(OUTPUT_FILE, "w") as f:
    json.dump(events, f, indent=2)

print(f"successfully saved {len(events)} events to {OUTPUT_FILE}")