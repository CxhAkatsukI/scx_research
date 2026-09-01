#!/usr/bin/env bash
set -euo pipefail

BPF_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PROJECT_DIR=$(cd -- "$BPF_DIR/.." && pwd)
OUT_DIR="$PROJECT_DIR/target/bpf"
RUST_TARGET_DIR="$OUT_DIR/rust-target"

CLANG=${CLANG:-clang}
LLVM_LINK=${LLVM_LINK:-llvm-link}
OPT=${OPT:-opt}
LLC=${LLC:-llc}
RUSTC_BPF_TOOLCHAIN=${RUSTC_BPF_TOOLCHAIN:-1.97.1}
CARGO_DRIVER_TOOLCHAIN=${CARGO_DRIVER_TOOLCHAIN:-nightly}
RUSTC_BPF=${RUSTC_BPF:-$(rustup which --toolchain "$RUSTC_BPF_TOOLCHAIN" rustc)}

mkdir -p "$OUT_DIR"

VMLINUX_H=${VMLINUX_H:-$OUT_DIR/vmlinux.h}
if [[ ! -f "$VMLINUX_H" ]]; then
	bpftool btf dump file /sys/kernel/btf/vmlinux format c > "$VMLINUX_H"
fi
if ! grep -q "struct sched_ext_ops" "$VMLINUX_H"; then
	echo "error: $VMLINUX_H does not describe a sched_ext-enabled kernel" >&2
	exit 1
fi

rust_llvm=$($RUSTC_BPF -vV | awk -F': ' '/LLVM version/ { split($2, v, "."); print v[1] }')
clang_llvm=$($CLANG --version | sed -nE 's/.*version ([0-9]+).*/\1/p' | head -n 1)
link_llvm=$($LLVM_LINK --version | awk '/LLVM version/ { split($3, v, "."); print v[1]; exit }')
if [[ "$rust_llvm" != "$clang_llvm" || "$rust_llvm" != "$link_llvm" ]]; then
	echo "error: Rust, clang, and llvm-link must use the same LLVM major" >&2
	echo "       rustc=$rust_llvm clang=$clang_llvm llvm-link=$link_llvm" >&2
	exit 1
fi

$CLANG -target bpf -g -O2 -emit-llvm -c "$BPF_DIR/abi_shim.bpf.c" \
	-o "$OUT_DIR/abi_shim.bc" -include "$VMLINUX_H" -I/usr/include

RUSTC="$RUSTC_BPF" \
RUSTC_BOOTSTRAP=1 \
RUSTFLAGS="-C debuginfo=2" \
CARGO_TARGET_DIR="$RUST_TARGET_DIR" \
	cargo +"$CARGO_DRIVER_TOOLCHAIN" rustc \
	--manifest-path "$BPF_DIR/Cargo.toml" \
	--lib --release --target bpfel-unknown-none -Z build-std=core \
	-- --emit=llvm-bc

rust_bc=$(find "$RUST_TARGET_DIR/bpfel-unknown-none/release/build" \
	-type f -name 'sched_ext_verus_bpf-*.bc' -print -quit)
if [[ -z "$rust_bc" ]]; then
	echo "error: Rust LLVM bitcode was not produced" >&2
	exit 1
fi

$LLVM_LINK "$OUT_DIR/abi_shim.bc" "$rust_bc" -o "$OUT_DIR/combined.bc"
$OPT -O2 "$OUT_DIR/combined.bc" -o "$OUT_DIR/combined.opt.bc"
$LLC -march=bpfel -filetype=obj "$OUT_DIR/combined.opt.bc" \
	-o "$OUT_DIR/rust_minimal.bpf.o"

bpftool btf dump file "$OUT_DIR/rust_minimal.bpf.o" format raw \
	> "$OUT_DIR/rust_minimal.btf.txt"
grep -q "scx_bpf_dsq_insert" "$OUT_DIR/rust_minimal.btf.txt"
bpftool gen skeleton "$OUT_DIR/rust_minimal.bpf.o" name rust_minimal > /dev/null

echo "$OUT_DIR/rust_minimal.bpf.o"
