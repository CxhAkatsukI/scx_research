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

cargo run --quiet --manifest-path adapter/rustland_repro/Cargo.toml -- \
  --mode "${MODE}" \
  --recovery-delay-ms "${RECOVERY_DELAY_MS:-100}" \
  >"${ADAPTER_JSONL}" 2>"${ADAPTER_STDERR}" &
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

cargo run --quiet --bin problem1_workload -- \
  --progress-file "${PROGRESS_FILE}" \
  --stop-file "${STOP_FILE}" \
  --cpu-list "${PROBLEM1_WORKLOAD_CPU_LIST}" \
  --sched-ext \
  --write-every "${WORKLOAD_WRITE_EVERY:-10000}" \
  2>"${WORKLOAD_STDERR}" &
workload_pid="$!"

wait "${adapter_pid}" || true
touch "${STOP_FILE}"
wait "${workload_pid}" || true

echo "adapter_jsonl=${ADAPTER_JSONL}"
echo "adapter_stderr=${ADAPTER_STDERR}"
echo "workload_progress=${PROGRESS_FILE}"
echo "workload_stderr=${WORKLOAD_STDERR}"
echo "sched_ext_state_after=$(cat /sys/kernel/sched_ext/state 2>/dev/null || echo missing)"
