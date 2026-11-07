#!/usr/bin/env bash
set -euo pipefail

case ${1:-} in
    ''|--build-only|--disable-antistall) ;;
    *) echo "Usage: bash $0 [--build-only|--disable-antistall]" >&2; exit 2 ;;
esac
(( $# <= 1 && EUID != 0 )) || exit 2
systemd-detect-virt --vm >/dev/null || { echo 'VM only.' >&2; exit 2; }

scripts=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
work=$scripts/../.vm-run
scx=/sys/kernel/sched_ext
rev=e7b3a219617472cf78880ac424714e52c932db61
base=$work/scx-v1.1.4
src=$work/scx-stall-v1.1.4
binary=$work/stall-build/release/scx_layered
workload=$work/stall-workload
duration=${STALL_SECONDS:-90}
[[ $duration =~ ^[1-9][0-9]{1,2}$ ]] && (( duration >= 40 && duration <= 300 )) || {
    echo 'STALL_SECONDS must be between 40 and 300.' >&2; exit 2;
}
monitor_duration=$((duration - 5))

mkdir -p -- "$work"
work=$(cd -- "$work" && pwd -P)
base=$work/scx-v1.1.4
src=$work/scx-stall-v1.1.4
binary=$work/stall-build/release/scx_layered
workload=$work/stall-workload
run=$(mktemp -d "$work/stall.XXXXXX")
echo "Logs: $run"

main() {
    uname -a
    lscpu -e=CPU,NODE,CACHE,ONLINE
    if [[ ! -d $base ]]; then
        git clone --depth 1 --branch v1.1.4 https://github.com/sched-ext/scx.git "$base"
    fi
    [[ $(git -C "$base" rev-parse HEAD) == "$rev" ]]
    [[ -z $(git -C "$base" status --porcelain --untracked-files=no) ]]
    echo "Upstream: $rev"
    if [[ ! -d $src ]]; then
        git clone --quiet --shared "$base" "$src"
        git -C "$src" checkout --quiet --detach "$rev"
    fi
    [[ $(git -C "$src" rev-parse HEAD) == "$rev" ]]
    python3 "$scripts/stall.py" instrument "$base" "$src"
    cc -O2 -Wall -Wextra -o "$workload" "$scripts/stall_workload.c"
    (
        cd "$src"
        CARGO_TARGET_DIR="$work/stall-build" cargo build --locked --release -j 2 -p scx_layered
    )
    "$binary" --version
    if [[ ${1:-} == --build-only ]]; then
        echo 'Build only. Nothing loaded.'
        return
    fi

    [[ $(cat "$scx/state") == disabled ]] || { echo 'Another scheduler is active.'; return 1; }
    local spec=$scripts/stall-layers.json trace=$run/trace.log
    local loader monitor background status ops attached pid
    local -a load_pids=() target_pids=()
    local -a loader_cmd=(timeout --signal=INT --kill-after=8s "${duration}s"
                         "$binary" --interval 0.1 --stats 1)
    if [[ ${1:-} == --disable-antistall ]]; then
        loader_cmd+=(--disable-antistall)
        echo 'Experiment: anti-stall DISABLED (bounded comparison run).'
    else
        echo 'Experiment: anti-stall ENABLED (default 3 seconds, 15-second scan).'
    fi
    loader_cmd+=("file:$spec")

    sudo -S -p '' -v
    sudo -n /usr/bin/bash -c '
        [[ $(< /sys/kernel/sched_ext/state) == disabled ]] || exit 1
        exec "$@"
    ' stall-loader "${loader_cmd[@]}" </dev/null >"$trace" 2>&1 &
    loader=$!
    python3 "$scripts/stall.py" monitor "$trace" "$run/monitor.json" \
        "$monitor_duration" "$run/freeze" >"$run/monitor.log" 2>&1 &
    monitor=$!

    cleanup() {
        for pid in "${load_pids[@]:-}" "${target_pids[@]:-}"; do kill -TERM "$pid" 2>/dev/null || true; done
        if [[ -n ${background:-} ]]; then kill -TERM "$background" 2>/dev/null || true; fi
        if [[ -n ${monitor:-} ]]; then kill -TERM "$monitor" 2>/dev/null || true; fi
        if [[ -n ${loader:-} ]]; then kill -TERM "$loader" 2>/dev/null || true; fi
    }
    trap 'exit 130' INT TERM HUP
    trap cleanup EXIT

    attached=0
    for ((i = 0; i < 150; i++)); do
        kill -0 "$loader" 2>/dev/null || break
        ops=$(cat "$scx/root/ops" 2>/dev/null || true)
        if [[ $(cat "$scx/state") == enabled && $ops == layered_1.1.4_* ]]; then
            attached=1
            echo "Attached: $ops"
            break
        fi
        sleep 0.1
    done
    if (( attached )); then
        # Keep idle CPUs unavailable without adding work to the target layer.
        stress-ng --cpu-method loop --cpu-load-slice 5 --cpu "$(nproc)" \
            --cpu-load 95 --timeout "${duration}s" >"$run/other.log" 2>&1 &
        background=$!
        for ((i = 0; i < 4; i++)); do
            "$workload" stall-target "$duration" 80 >>"$run/target.log" 2>&1 &
            target_pids+=("$!")
        done
        echo "Long-lived targets: ${target_pids[*]}"
        while kill -0 "$loader" 2>/dev/null && [[ ! -e $run/freeze ]]; do
            load_pids=()
            for ((i = 0; i < $(nproc) * 2 && i < 16; i++)); do
                "$workload" stall-load 2 95 >>"$run/load.log" 2>&1 &
                load_pids+=("$!")
            done
            for pid in "${load_pids[@]}"; do wait "$pid" || true; done
            load_pids=()
            [[ -e $run/freeze ]] || sleep 2
        done
        if [[ -e $run/freeze ]]; then
            echo 'Target race matched: no further heavy-load/config stimulation.'
        else
            echo 'No target match before loader exit.'
        fi
    fi

    status=0
    wait "$loader" || status=$?
    loader=''
    if [[ -n ${background:-} ]]; then
        kill -TERM "$background" 2>/dev/null || true
        wait "$background" || true
        background=''
    fi
    for pid in "${target_pids[@]:-}"; do
        kill -TERM "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done
    target_pids=()
    wait "$monitor" || true
    monitor=''
    echo "Loader status: $status; final sched_ext state: $(cat "$scx/state")"
    local watchdog=0
    if grep -q 'runnable task stall (stall-target' "$trace"; then
        watchdog=1
    fi
    if (( ! attached )) || [[ $(cat "$scx/state") != disabled ]] ||
       [[ $status != 124 && $watchdog == 0 ]]; then
        echo 'INCONCLUSIVE: scheduler did not finish the bounded run. Loader output:'
        tail -n 40 "$trace"
        return 1
    fi
    if (( watchdog )); then
        echo 'CONFIRMED: the kernel sched_ext watchdog rescued a causally matched target.'
    fi
    python3 "$scripts/stall.py" report "$trace"
    cat "$run/monitor.log"
    python3 - "$run/monitor.json" <<'PY'
import json
import sys
with open(sys.argv[1]) as f:
    result = json.load(f)
if not result["target_matches"]:
    print("INCONCLUSIVE: no causal race match for the long-lived target.")
    raise SystemExit(1)
PY
}

main "$@" 2>&1 | tee "$run/run.log"
