#!/usr/bin/env bash
set -euo pipefail

case ${1:-} in
    ''|--build-only) ;;
    *) echo "Usage: bash $0 [--build-only]" >&2; exit 2 ;;
esac
(( $# <= 1 && EUID != 0 )) || exit 2
systemd-detect-virt --vm >/dev/null || { echo 'VM only.' >&2; exit 2; }

scripts=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
work=$scripts/../.vm-run
scx=/sys/kernel/sched_ext
rev=e7b3a219617472cf78880ac424714e52c932db61
base=$work/scx-v1.1.4
src=$work/scx-race-v1.1.4
binary=$work/race-target/release/scx_layered
duration=${RACE_SECONDS:-75}
[[ $duration =~ ^[1-9][0-9]{1,2}$ ]] && (( duration >= 20 && duration <= 300 )) || {
    echo 'RACE_SECONDS must be between 20 and 300.' >&2; exit 2;
}

mkdir -p -- "$work"
work=$(cd -- "$work" && pwd -P)
base=$work/scx-v1.1.4
src=$work/scx-race-v1.1.4
binary=$work/race-target/release/scx_layered
run=$(mktemp -d "$work/race.XXXXXX")
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
    python3 "$scripts/race.py" instrument "$base" "$src"
    (
        cd "$src"
        CARGO_TARGET_DIR="$work/race-target" cargo build --locked --release -j 2 -p scx_layered
    )
    "$binary" --version
    if [[ ${1:-} == --build-only ]]; then
        echo 'Build only. Nothing loaded.'
        return
    fi

    command -v stress-ng >/dev/null
    [[ $(cat "$scx/state") == disabled ]] || { echo 'Another scheduler is active.'; return 1; }
    local spec=$scripts/race-layers.json trace=$run/trace.log loader light status ops attached workers
    local workload_failed=0
    # The guest lacks AVX, which some stress-ng CPU methods require.
    local -a load=(stress-ng --cpu-method loop --cpu-load-slice 5)
    workers=$(( $(nproc) * 2 ))
    (( workers <= 16 )) || workers=16

    sudo -v
    sudo -n /usr/bin/bash -c '
        [[ $(< /sys/kernel/sched_ext/state) == disabled ]] || exit 1
        exec timeout --signal=INT --kill-after=8s "$1" "$2" --interval 0.1 --stats 1 "file:$3"
    ' race-loader "${duration}s" "$binary" "$spec" </dev/null >"$trace" 2>&1 &
    loader=$!
    trap 'exit 130' INT TERM HUP
    # On cancellation, stop the bounded loader and the continuous wakeups.
    trap 'if [[ -n ${light:-} ]]; then kill -TERM "$light" 2>/dev/null || true; fi
          if [[ -n ${loader:-} ]]; then kill -TERM "$loader" 2>/dev/null || true; fi' EXIT

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
        # Keep wakeups during both growth and shrink, not just while fully loaded.
        "${load[@]}" --cpu 2 --cpu-load 15 --timeout "${duration}s" >"$run/light.log" 2>&1 &
        light=$!
        while kill -0 "$loader" 2>/dev/null; do
            if ! "${load[@]}" --cpu "$workers" --cpu-load 95 --timeout 2s; then
                echo 'INCONCLUSIVE: heavy workload failed.'
                workload_failed=1
                break
            fi
            sleep 2
        done
        if kill -TERM "$light" 2>/dev/null; then
            wait "$light" || true
        elif ! wait "$light"; then
            echo "INCONCLUSIVE: light workload failed; see $run/light.log."
            workload_failed=1
        fi
        light=''
    fi
    status=0
    wait "$loader" || status=$?
    loader=''
    echo "Loader status: $status; final sched_ext state: $(cat "$scx/state")"
    if (( ! attached )) || [[ $status != 124 || $(cat "$scx/state") != disabled ]]; then
        echo 'INCONCLUSIVE: scheduler did not finish the bounded run. Loader output:'
        tail -n 40 "$trace"
        return 1
    fi
    (( ! workload_failed )) || return 1
    python3 "$scripts/race.py" report "$trace"
}

main "$@" 2>&1 | tee "$run/run.log"
