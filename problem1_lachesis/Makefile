# SPDX-License-Identifier: GPL-2.0
PROG := p1_lachesis
SRC := src/sched/main.rs
ROOT_DIR := $(CURDIR)
STAGE1 := $(abspath ../lachesis_step1)
BUILD_DIR := $(CURDIR)/build
VERUS_DIR := $(STAGE1)/dep/verus
# Toolchain sources are shared; all generated outputs remain isolated.
HOSTDIR := $(BUILD_DIR)/host
DEPDIR := $(BUILD_DIR)/rust-deps
override LIB_CRATES := p1_protocol lachesis_runtime_trusted p1_runtime
override NOCHEAT_CRATES := p1_protocol
p1_protocol_DIR := $(CURDIR)/src/protocol
lachesis_runtime_trusted_DIR := $(CURDIR)/src/trusted
p1_runtime_DIR := $(CURDIR)/src/runtime
KEEP_SYMS := p1_ops p1_select_cpu p1_enqueue p1_dispatch p1_running p1_disable p1_exit_task p1_init p1_config P1 tickets _LICENSE
include $(STAGE1)/src/toolchain/rules.mk
VERUS_ERASE_FLAGS += --check-cfg 'cfg(verus_verify_core)' --check-cfg 'cfg(verus_no_vstd)'

.PHONY: setup
setup:
	bash scripts/setup.sh
	@mkdir -p $(DEPDIR)
	cp $(STAGE1)/build/rust-deps/libcore.rlib $(STAGE1)/build/rust-deps/libcore.rmeta $(DEPDIR)/
	cp $(STAGE1)/build/rust-deps/libcompiler_builtins.rlib $(STAGE1)/build/rust-deps/libcompiler_builtins.rmeta $(DEPDIR)/

.PHONY: smoke-loader vm-smoke
smoke-loader: all
	$(BPFTOOL) btf dump file $(OUT)/$(PROG).o format c > $(OUT)/p1.types.h
	$(BPFTOOL) gen skeleton $(OUT)/$(PROG).o name p1 > $(OUT)/p1.skel.h
	$(CC) -O2 -g -Wall -Wextra -Werror -I$(OUT) tests/vm_smoke.c -lbpf -o $(OUT)/vm-smoke

vm-smoke: smoke-loader
	bash scripts/vm-smoke.sh $(VMLINUX)

.PHONY: check mutants test
check: verify test mutants
mutants: verify
	VERUS=$(VERUS) python3 scripts/mutants.py
test: verify
	$(RUSTC) --edition 2021 -O tests/word.rs --extern p1_protocol=$(HOSTDIR)/verus/libp1_protocol.rlib \
		-L dependency=$(HOSTDIR)/verus -L dependency=$(VERUS_DIR)/source/target-verus/release -o $(OUT)/word-test
	$(OUT)/word-test

