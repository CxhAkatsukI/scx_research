#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0
# Explicit hardware test entry. The smoke loader otherwise refuses host loading.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} == --help ]]; then
    echo 'sudo bash scripts/load.sh: bounded partial-switch test on CPUs 0, 1, 2'
    echo 'Build with make smoke-loader against this machine BTF first.'
    exit 0
fi
[[ $EUID == 0 ]] || { echo 'Run with sudo on the test machine' >&2; exit 1; }
[[ $(cat /sys/kernel/sched_ext/state) == disabled ]]
exec build/p1_lachesis/vm-smoke --allow-host
