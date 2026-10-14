#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    printf '%s\n' \
        'Usage: sudo ./scripts/load.sh [--duration SECS] [--interval SECS]' \
        'Run make build on the target machine first. Default duration: 5 seconds.' \
        'Use --duration 0 to run until Ctrl-C. The loader detaches on exit.'
    exit 0
fi
if [[ $EUID != 0 ]]; then
    echo 'Run this script with sudo on the test machine.' >&2
    exit 1
fi
state=/sys/kernel/sched_ext/state
if [[ ! -r $state ]]; then
    echo 'The running kernel does not expose sched_ext.' >&2
    exit 1
fi
if [[ $(<"$state") != disabled ]]; then
    echo 'Another sched_ext scheduler is active; stop it before this test.' >&2
    exit 1
fi
out="$PWD/build/lachesis"
if [[ ! -x $out/lachesis || ! -f $out/lachesis.o ]]; then
    echo 'Missing build artifacts. Run make build first.' >&2
    exit 1
fi
# The upstream loader defaults to QEMU only; this explicit test entry allows hardware.
exec "$out/lachesis" --allow-host --obj "$out/lachesis.o" "$@"
