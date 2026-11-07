#!/usr/bin/env bash
set -euo pipefail

case ${1:-} in
    ''|--preflight) ;;
    *) echo "Usage: bash $0 [--preflight]" >&2; exit 2 ;;
esac
(( $# <= 1 )) || exit 2
(( EUID != 0 )) || { echo 'Run as the VM user, not with sudo.' >&2; exit 2; }

case_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
run_dir=$case_dir/.vm-run
scx=/sys/kernel/sched_ext
release=v1.1.4
revision=e7b3a219617472cf78880ac424714e52c932db61
bpf_path=scheds/rust/scx_layered/src/bpf/main.bpf.c
audited_blob=2a0f07a347840185cd7eeea5d256d811a1f91911

die() { echo "FAIL: $*" >&2; exit 1; }

main() {
    local virt source state binary
    date -Is
    uname -a
    virt=$(systemd-detect-virt --vm) || die 'Run this test inside the VM.'
    echo "VM: $virt"
    lscpu -e=CPU,NODE,CACHE,ONLINE
    [[ -r /sys/kernel/btf/vmlinux ]] || die 'Kernel BTF is unavailable.'
    state=$(cat "$scx/state") || die 'sched_ext sysfs is unavailable.'
    echo "Initial sched_ext state: $state"
    [[ $state == disabled ]] || die 'Another sched_ext scheduler is active.'

    local tool
    for tool in git cargo rustc clang make pkg-config timeout sudo; do
        command -v "$tool" >/dev/null || die "Missing build/runtime tool: $tool"
    done
    cargo --version
    rustc --version
    clang --version
    if [[ ${1:-} == --preflight ]]; then
        echo 'PASS: preflight only; nothing loaded.'
        return
    fi

    source=$run_dir/scx-$release
    if [[ ! -d $source ]]; then
        git clone --depth 1 --branch "$release" https://github.com/sched-ext/scx.git "$source"
    fi
    [[ $(git -C "$source" rev-parse HEAD) == "$revision" ]] || die 'Unexpected upstream revision.'
    [[ -z $(git -C "$source" status --porcelain) ]] || die 'Upstream checkout has local changes.'
    [[ $(git hash-object "$source/$bpf_path") == "$audited_blob" ]] || die 'BPF source differs from the audit.'
    printf 'Upstream: %s %s\nAudited BPF blob: %s\n' "$release" "$revision" "$audited_blob"
    (
        cd "$source"
        CARGO_TARGET_DIR="$run_dir/target" cargo build --locked --release -j 2 -p scx_layered
    )
    binary=$run_dir/target/release/scx_layered
    "$binary" --version

    echo 'Requesting sudo for the 15-second VM attach test.'
    sudo -v
    sudo -n /usr/bin/bash -s -- "$binary" "$scx" <<'ROOT'
set -euo pipefail
binary=$1
scx=$2
if [[ $(cat "$scx/state") != disabled ]]; then
    echo 'FAIL: sched_ext is no longer disabled; refusing to attach.' >&2
    exit 1
fi

# Sample sysfs while timeout bounds the loader, including failed attaches.
start=$(date +%s)
timeout --signal=INT --kill-after=5s 15s "$binary" --run-example --stats 1 &
runner=$!
trap 'kill -TERM "$runner" 2>/dev/null || true; exit 130' INT TERM HUP
saw_layered=0
last=''
while kill -0 "$runner" 2>/dev/null; do
    state=$(cat "$scx/state")
    ops=$(cat "$scx/root/ops" 2>/dev/null || true)
    if [[ "$state/$ops" != "$last" ]]; then
        printf '%s sched_ext state=%s ops=%s\n' "$(date -Is)" "$state" "${ops:--}"
        last=$state/$ops
    fi
    if [[ $state == enabled && $ops == layered ]]; then
        saw_layered=1
    fi
    sleep 0.2
done
status=0
wait "$runner" || status=$?
for ((i = 0; i < 25; i++)); do
    [[ $(cat "$scx/state") == disabled ]] && break
    sleep 0.2
done
final_state=$(cat "$scx/state")
echo "Loader status: $status; observed layered: $saw_layered; final sched_ext state: $final_state"
journalctl -k --since "@$start" --no-pager -n 80 || true
if [[ $status != 124 || $saw_layered != 1 || $final_state != disabled ]]; then
    echo 'FAIL: expected bounded attach and clean detach.' >&2
    exit 1
fi
echo 'PASS: layered attached and detached. Drain race was not tested.'
ROOT
}

mkdir -p -- "$run_dir"
log=$(mktemp "$run_dir/run.XXXXXX.log")
printf 'Log: %s\n' "$log"
main "$@" 2>&1 | tee "$log"
