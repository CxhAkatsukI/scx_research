STAGE1 ?= ../../lachesis_step1
VERUS_ROOT ?= $(abspath $(STAGE1)/dep/verus/source/target-verus/release)
VERUS ?= $(VERUS_ROOT)/verus
RUSTC ?= $(HOME)/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc
ERASE := --edition=2021 --check-cfg 'cfg(verus_keep_ghost)' --check-cfg 'cfg(host_replay)' \
 --check-cfg 'cfg(verus_verify_core)' --check-cfg 'cfg(verus_no_vstd)' --check-cfg 'cfg(verus_keep_ghost_body)' \
 --extern verus_builtin_macros=$(VERUS_ROOT)/libverus_builtin_macros.so -L dependency=$(VERUS_ROOT)
SOURCES := $(wildcard src/*.rs)

.PHONY: check evidence verify replay mutants clean
check: evidence verify replay mutants
evidence:
	python3 scripts/evidence.py
verify: build/verify.log
	@cat $<
build/verify.log: $(SOURCES) $(VERUS)
	@mkdir -p build
	$(VERUS) --crate-type=lib --crate-name layered_case src/lib.rs > $@ 2>&1 || { cat $@; rm -f $@; exit 1; }
	@rg 'verification results:: .* verified, 0 errors' $@
build/liblayered_case.rlib: $(SOURCES)
	@mkdir -p build
	$(RUSTC) $(ERASE) --cfg host_replay --crate-type=rlib --crate-name layered_case src/lib.rs -o $@
build/source.o: scripts/evidence.py tests/oracle.h upstream.json | evidence
	$(CC) -std=gnu11 -Wall -Wextra -Werror -Wno-unused-function -O2 -Itests -c build/source.c -o $@
build/replay: tests/replay.rs build/liblayered_case.rlib build/source.o
	$(RUSTC) --edition=2021 $< --extern layered_case=build/liblayered_case.rlib -L dependency=$(VERUS_ROOT) \
	 -C link-arg=build/source.o -o $@
replay: build/replay
	./build/replay
mutants: verify
	python3 scripts/mutants.py $(VERUS)
clean:
	rm -rf build
