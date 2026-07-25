#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-deterministic}"
RUN_ID="$(date +%Y%m%d_%H%M%S)"
OUT_DIR="${OUT_DIR:-traces/vm_rustland_${MODE}_${RUN_ID}}"
PROGRESS_FILE="${OUT_DIR}/workload.progress"
STOP_FILE="${OUT_DIR}/workload.stop"
ADAPTER_JSONL="${OUT_DIR}/adapter.jsonl"
ADAPTER_STDERR="${OUT_DIR}/adapter.stderr"
WORKLOAD_STDERR="${OUT_DIR}/workload.stderr"
WORKLOAD_LIVE="${OUT_DIR}/workload.live"

if [[ "${EUID}" -ne 0 ]]; then
  echo "run this VM harness with sudo; sched-ext loading and SCHED_EXT opt-in require privilege" >&2
  exit 1
fi

case "${MODE}" in
  report|deterministic|stochastic) ;;
  *)
    echo "usage: $0 [report|deterministic|stochastic]" >&2
    exit 2
    ;;
esac

mkdir -p "${OUT_DIR}"

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
export CARGO_TARGET_DIR="${TARGET_DIR}"
WORKLOAD_BIN="${TARGET_DIR}/debug/problem1_workload"
ADAPTER_BIN="${TARGET_DIR}/debug/problem1_stranded_draining_rustland"

cargo run --quiet --bin problem1_vm_preflight
eval "$(cargo run --quiet --bin problem1_vm_preflight -- --export-env)"

cargo build --quiet --bin problem1_workload
cargo build --quiet --manifest-path adapter/rustland_repro/Cargo.toml

adapter_pid=""
workload_pid=""

cleanup() {
  touch "${STOP_FILE}" || true
  if [[ -n "${workload_pid}" ]] && kill -0 "${workload_pid}" 2>/dev/null; then
    wait "${workload_pid}" || true
  fi
  if [[ -n "${adapter_pid}" ]] && kill -0 "${adapter_pid}" 2>/dev/null; then
    kill -INT "${adapter_pid}" || true
    wait "${adapter_pid}" || true
  fi
}
trap cleanup EXIT INT TERM

adapter_args=(
  --mode "${MODE}"
  --recovery-delay-ms "${RECOVERY_DELAY_MS:-100}"
  --max-runtime-ms "${MAX_RUNTIME_MS:-5000}"
)
if [[ "${ADAPTER_DEBUG:-0}" == "1" ]]; then
  adapter_args+=(--debug)
fi

"${ADAPTER_BIN}" "${adapter_args[@]}" >"${ADAPTER_JSONL}" 2>"${ADAPTER_STDERR}" &
adapter_pid="$!"

for _ in $(seq 1 100); do
  if [[ -r /sys/kernel/sched_ext/state ]] && grep -q enabled /sys/kernel/sched_ext/state; then
    break
  fi
  sleep 0.05
done

if ! grep -q enabled /sys/kernel/sched_ext/state; then
  echo "sched_ext did not become enabled; see ${ADAPTER_STDERR}" >&2
  exit 1
fi

"${WORKLOAD_BIN}" \
  --progress-file "${PROGRESS_FILE}" \
  --stop-file "${STOP_FILE}" \
  --cpu-list "${PROBLEM1_WORKLOAD_CPU_LIST}" \
  --sched-ext \
  --initial-sleep-ms "${WORKLOAD_INITIAL_SLEEP_MS:-1}" \
  --write-every "${WORKLOAD_WRITE_EVERY:-10000}" \
  2>"${WORKLOAD_STDERR}" &
workload_pid="$!"

sleep "${LIVE_STATUS_DELAY_SEC:-0.2}"
{
  echo "sched_ext_sysfs:"
  for file in /sys/kernel/sched_ext/state /sys/kernel/sched_ext/switch_all /sys/kernel/sched_ext/nr_rejected /sys/kernel/sched_ext/enable_seq; do
    printf "%s=" "${file}"
    cat "${file}" 2>/dev/null || echo "unreadable"
  done

  echo
  echo "ps:"
  ps -o pid,tid,cls,policy,psr,comm -L -p "${workload_pid}" || true

  echo
  echo "chrt:"
  chrt -p "${workload_pid}" || true

  echo
  echo "proc_status:"
  grep -E '^(Name|State|Pid|PPid|Threads|Cpus_allowed_list|voluntary_ctxt_switches|nonvoluntary_ctxt_switches):' "/proc/${workload_pid}/status" || true

  echo
  echo "proc_sched:"
  sed -n '1,120p' "/proc/${workload_pid}/sched" || true
} >"${WORKLOAD_LIVE}" 2>&1

wait "${adapter_pid}" || true
touch "${STOP_FILE}"
wait "${workload_pid}" || true

echo "adapter_jsonl=${ADAPTER_JSONL}"
echo "adapter_stderr=${ADAPTER_STDERR}"
echo "workload_progress=${PROGRESS_FILE}"
echo "workload_stderr=${WORKLOAD_STDERR}"
echo "workload_live=${WORKLOAD_LIVE}"
echo "adapter_pid=${adapter_pid}"
echo "workload_pid=${workload_pid}"
echo "sched_ext_state_after=$(cat /sys/kernel/sched_ext/state 2>/dev/null || echo missing)"
