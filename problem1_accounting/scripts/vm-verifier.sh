#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Guest-only verifier pass. Does not attach a scheduler.
set -euo pipefail
object=${1:?object required}
out=${2:?output directory required}
expected_btf=${3:?build BTF hash required}
[[ $(cat /sys/class/dmi/id/sys_vendor) == QEMU ]] || {
    echo 'Refusing to run outside QEMU' >&2
    exit 1
}
pin=/sys/fs/bpf/problem1_accounting_verify
[[ ! -e "$pin" ]] || { echo 'Verifier pin path already exists' >&2; exit 1; }
mountpoint -q /sys/fs/bpf || mount -t bpf bpf /sys/fs/bpf
trap 'rm -rf "$pin"' EXIT
uname -a
rm -f "$out/vmlinux.btf"
cp /sys/kernel/btf/vmlinux "$out/vmlinux.btf"
uname -r >"$out/kernel-release.txt"
read -r actual_btf _ < <(sha256sum "$out/vmlinux.btf")
[[ $actual_btf == "$expected_btf" ]] || {
    echo "BTF differs: rebuild with VMLINUX=$out/vmlinux.btf and retry" >&2
    exit 1
}
before=$(cat /sys/kernel/sched_ext/enable_seq)
bpftool -d prog loadall "$object" "$pin" >"$out/verifier.log" 2>&1 || {
    tail -80 "$out/verifier.log"
    exit 1
}
[[ $(cat /sys/kernel/sched_ext/state) == disabled ]]
[[ $(cat /sys/kernel/sched_ext/enable_seq) == "$before" ]]
echo 'OK: all BPF programs accepted; scheduler remained disabled'
touch "$out/passed"
