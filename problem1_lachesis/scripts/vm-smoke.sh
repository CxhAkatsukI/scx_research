#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0
# Loading happens only in the guest branch, with explicit VM/BTF guards.
set -euo pipefail
cd "$(dirname "$0")/.."
out=$PWD/build/vm
loader=$PWD/build/p1_lachesis/vm-smoke
if [[ ${1:-} == --guest ]]; then
    [[ $(cat /sys/class/dmi/id/sys_vendor) == QEMU ]]
    [[ $(cat /sys/kernel/sched_ext/state) == disabled ]]
    read -r actual _ < <(sha256sum /sys/kernel/btf/vmlinux)
    [[ $actual == "$2" ]] || { echo 'Build BTF differs from guest BTF' >&2; exit 1; }
    uname -a
    "$loader" > "$out/smoke.log" 2>&1 || { cat "$out/smoke.log"; exit 1; }
    cat "$out/smoke.log"
    [[ $(cat /sys/kernel/sched_ext/state) == disabled ]]
    echo 'OK: scheduler detached; sched_ext state=disabled'
    touch "$out/passed"
    exit 0
fi
btf=${1:?pass the BTF used to build the object}
vng=${VNG:-$PWD/../lachesis_step1/build/vm-tools/bin/vng}
kernel=${KERNEL:-/boot/vmlinuz-linux}
mkdir -p "$out"
rm -f "$out/passed"
read -r hash _ < <(sha256sum "$btf")
export PATH="$(dirname "$vng"):$PATH"
timeout --kill-after=5 90 "$vng" --run "$kernel" --user root --cpus 4 --memory 2G \
    --disable-microvm --rwdir "$out" -- \
    bash "$PWD/scripts/vm-smoke.sh" --guest "$hash" > "$out/run.log" 2>&1 || {
        tail -100 "$out/run.log"
        exit 1
    }
cat "$out/run.log"
test -f "$out/passed"
