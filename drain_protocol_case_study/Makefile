STAGE1 ?= ../lachesis_step1
VERUS_ROOT ?= $(abspath $(STAGE1)/dep/verus/source/target-verus/release)
VERUS ?= $(VERUS_ROOT)/verus
RUSTC ?= $(HOME)/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/rustc
MACROS := $(VERUS_ROOT)/libverus_builtin_macros.so
SOURCES := $(wildcard src/*.rs)
ERASE := --edition=2021 --check-cfg 'cfg(verus_keep_ghost)' --check-cfg 'cfg(host_replay)' \
 --check-cfg 'cfg(verus_verify_core)' --check-cfg 'cfg(verus_no_vstd)' --check-cfg 'cfg(verus_keep_ghost_body)' \
 --extern verus_builtin_macros=$(MACROS) -L dependency=$(VERUS_ROOT)

.PHONY: check fetch verify replay mutants clean
check: fetch verify replay mutants
fetch:
	python3 scripts/fetch.py
verify: build/verify.log
	@cat $<
build/verify.log: $(SOURCES) $(VERUS)
	@mkdir -p build
	$(VERUS) --crate-type=lib --crate-name drain_case src/lib.rs > $@ 2>&1 || { cat $@; rm -f $@; exit 1; }
	@rg 'verification results:: .* verified, 0 errors' $@
build/libdrain_case.rlib: $(SOURCES)
	@mkdir -p build
	$(RUSTC) $(ERASE) --cfg host_replay --crate-type=rlib --crate-name drain_case src/lib.rs -o $@
replay: build/replay
	./build/replay
build/oracle.stamp: scripts/extract.py scripts/fetch.py tests/oracle.h upstream.json | fetch
	python3 scripts/extract.py
	$(CC) -std=gnu11 -Wall -Wextra -Werror -Wno-unused-function -O2 -Itests -c build/before.c -o build/before.o
	$(CC) -std=gnu11 -Wall -Wextra -Werror -Wno-unused-function -O2 -Itests -c build/after.c -o build/after.o
	touch $@
build/replay: tests/replay.rs build/libdrain_case.rlib build/oracle.stamp
	$(RUSTC) --edition=2021 $< --extern drain_case=build/libdrain_case.rlib -L dependency=$(VERUS_ROOT) \
	 -C link-arg=build/before.o -C link-arg=build/after.o -o $@
mutants: verify
	python3 scripts/mutants.py $(VERUS)
clean:
	rm -rf build
