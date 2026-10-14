SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

ROOT_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
VERUS_DIR := $(ROOT_DIR)/dep/verus
BUILD_DIR := $(ROOT_DIR)/build
VERUS_SOURCE := $(VERUS_DIR)/source
VERUS_READY := $(BUILD_DIR)/verus-ready.stamp
VMLINUX ?= /sys/kernel/btf/vmlinux
KERNEL ?= /boot/vmlinuz-linux
WORKLOAD_SECS ?= 5
SCHED_MAKE = $(MAKE) -C $(ROOT_DIR)/src/sched VMLINUX=$(VMLINUX)

.PHONY: help setup verus verify check build lachesis vm-run lachesis-run clean rust-project
help:
	@printf '%s\n' \
	  'make setup       Fetch pinned Verus and install its Rust toolchain' \
	  'make verify      Build Verus and check the original scheduler' \
	  'make check       Verify, then reject two broken policy variants' \
	  'make build       Verify and build the BPF object and loader' \
	  'make vm-run      Build and test in QEMU (requires virtme-ng)' \
	  'make clean       Remove scheduler outputs (keep toolchain caches)' \
	  'make rust-project  Generate rust-analyzer project metadata' \
	  '' \
	  'Use VMLINUX=/path/to/target/vmlinux for an offline kernel build.' \
	  'On the test machine: sudo ./scripts/load.sh --duration 10'

setup:
	./scripts/setup.sh

verus: $(VERUS_READY)

# The driver can exist before vstd finishes; stamp only a complete build.
$(VERUS_READY):
	@test -f $(VERUS_SOURCE)/Cargo.toml || { echo 'Run make setup first.' >&2; exit 1; }
	@test -x $(VERUS_SOURCE)/z3 || { echo 'Run make setup first (Z3 missing).' >&2; exit 1; }
	cd $(VERUS_SOURCE) && source ../tools/activate && \
		RUSTC_BOOTSTRAP=1 vargo build --release --vstd-no-std --vstd-no-alloc
	@mkdir -p $(BUILD_DIR)
	@touch $@

verify: verus
	$(SCHED_MAKE) verify

check: verify
	python3 scripts/check-contracts.py

build lachesis: verus
	$(SCHED_MAKE)

vm-run lachesis-run: build
	$(ROOT_DIR)/src/sched/vm-run.sh \
		$(BUILD_DIR)/lachesis/lachesis $(BUILD_DIR)/lachesis/lachesis.o \
		$(KERNEL) $(BUILD_DIR)/lachesis $(WORKLOAD_SECS)

rust-project: verus
	$(SCHED_MAKE) rust-project

clean:
	$(SCHED_MAKE) clean
