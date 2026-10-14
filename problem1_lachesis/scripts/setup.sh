#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0
set -euo pipefail
cd "$(dirname "$0")/.."
pin=3120c6f188e1d96562277c4bba38e5e6961e3066
mkdir -p dep
if [[ ! -d dep/lachesis/.git ]]; then
    local_copy=../lachesis_step1/dep/lachesis-upstream
    if [[ -d $local_copy/.git ]]; then
        git clone --no-hardlinks "$local_copy" dep/lachesis
    else
        git clone https://github.com/kkdwvd/lachesis.git dep/lachesis
    fi
fi
git -C dep/lachesis checkout --detach "$pin"
# Verus and the portable rust-bpf build tools are reused, not copied or edited.
test -x ../lachesis_step1/dep/verus/source/target-verus/release/verus || {
    echo 'Run make setup verus in ../lachesis_step1 first.' >&2
    exit 1
}
