.PHONY: check test callbacks verify mutants build bpf vm-verify vm-smoke
check: test callbacks mutants
test:
	cargo test
callbacks:
	python3 scripts/check_callbacks.py
verify:
	python3 scripts/verify.py
mutants:
	python3 scripts/verify.py --mutants
build:
	cargo build --release
bpf: verify
	python3 scripts/build_bpf.py
vm-verify:
	python3 scripts/check_vm.py
vm-smoke:
	python3 scripts/build_vm_smoke.py
	python3 scripts/check_vm.py --smoke
