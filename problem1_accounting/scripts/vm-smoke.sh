#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
set -euo pipefail
loader=${1:?loader required}
out=${2:?output directory required}
expected_btf=${3:?build BTF hash required}
[[ $(cat /sys/class/dmi/id/sys_vendor) == QEMU ]]
[[ $(cat /sys/kernel/sched_ext/state) == disabled ]]
read -r actual_btf _ < <(sha256sum /sys/kernel/btf/vmlinux)
[[ $actual_btf == "$expected_btf" ]] || { echo 'BTF differs: run make vm-verify first' >&2; exit 1; }
uname -a
"$loader" >"$out/smoke.log" 2>&1 || {
    cat "$out/smoke.log"
    dmesg | tail -60
    exit 1
}
cat "$out/smoke.log"
[[ $(cat /sys/kernel/sched_ext/state) == disabled ]]
echo 'OK: scheduler detached; sched_ext state=disabled'
touch "$out/smoke-passed"
