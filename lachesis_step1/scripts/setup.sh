#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# This public dependency is ignored by the parent research repository.
revision=5f6eb59f7a5527bf5c0c35995a4d6d1c410981a1
rust_version=1.98.0
for tool in git rustup make cc pkg-config python3; do
    command -v "$tool" >/dev/null || { echo "Missing tool: $tool" >&2; exit 1; }
done

if [[ ! -d dep/verus/.git ]]; then
    if [[ -e dep/verus ]]; then
        echo 'dep/verus already exists without Git metadata; inspect it first.' >&2
        exit 1
    fi
    mkdir -p dep
    scratch=$(mktemp -d "$PWD/dep/.verus-XXXXXX")
    trap 'rm -rf "$scratch"' EXIT
    git init -q "$scratch/source"
    git -C "$scratch/source" remote add origin https://github.com/verus-lang/verus.git
    git -C "$scratch/source" fetch --depth 1 origin "$revision"
    git -C "$scratch/source" checkout -q --detach FETCH_HEAD
    mv "$scratch/source" dep/verus
elif [[ $(git -C dep/verus rev-parse HEAD) != "$revision" ]]; then
    echo 'Unexpected Verus revision; refusing to replace local sources.' >&2
    exit 1
fi

# Reuse a complete installation without contacting a distribution mirror.
installed=$(rustup component list --toolchain "$rust_version" --installed 2>/dev/null || true)
need_rust=0
for component in cargo rustc rust-std rust-src rustc-dev llvm-tools rustfmt; do
    if ! grep -Eq "^${component}(-|$)" <<< "$installed"; then
        need_rust=1
    fi
done
if ((need_rust)); then
    RUSTUP_DIST_SERVER=${RUSTUP_DIST_SERVER:-https://static.rust-lang.org} \
        rustup toolchain install "$rust_version" --profile minimal \
        --component rust-src,rustc-dev,llvm-tools,rustfmt
fi

# The wheel contains the same pinned Z3 executable and supports older glibc.
if [[ ! -x dep/verus/source/z3 ]]; then
    mkdir -p build/downloads
    python3 -m pip download --no-deps --only-binary=:all: \
        z3-solver==4.16.0.0 --dest build/downloads
    python3 - <<'PY'
from pathlib import Path
from zipfile import ZipFile

wheels = list(Path("build/downloads").glob("z3_solver-4.16.0.0-*.whl"))
if len(wheels) != 1:
    raise SystemExit("Expected exactly one Z3 wheel for this platform")
with ZipFile(wheels[0]) as wheel:
    binary = next(name for name in wheel.namelist() if name.endswith("/bin/z3"))
    dest = Path("dep/verus/source/z3")
    dest.write_bytes(wheel.read(binary))
    dest.chmod(0o755)
PY
fi
dep/verus/source/z3 --version
printf '%s\n' 'Dependencies ready. Run make check, then make build.'
