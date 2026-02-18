# SCX Research Playground

Minimal experiments with Linux scheduler extensions (SCX) using a toy BPF scheduler and a handful of synthetic workloads for stress testing. The repository doubles as a scratchpad for visualizations and package management helpers used while iterating on SCX research.

## Table of Contents
- [Project Overview](#project-overview)
- [Repository Layout](#repository-layout)
- [Requirements](#requirements)
- [Build and Load the SCX Scheduler](#build-and-load-the-scx-scheduler)
- [Workload Generators](#workload-generators)
- [Visualization Sandbox](#visualization-sandbox)
- [Troubleshooting](#troubleshooting)

## Project Overview
The scheduler under `scheduler/scx_simple.bpf.c` implements the smallest possible SCX data-structure queue (DSQ) scheduler: the `simple_enqueue()` hook dispatches everything into a single shared DSQ (ID 0) and `simple_dispatch()` simply consumes tasks FIFO-style. The workloads under `workloads/` generate a mix of CPU bound and mixed IO/CPU traffic so you can observe how SCX reacts while you iterate on different queueing ideas.

## Repository Layout
```
.
├── scheduler/        # clang/bpftool build for the SCX BPF object
│   ├── scx_simple.bpf.c
│   └── cachyos-repo/ # upstream script for enabling CachyOS pacman repos
├── workloads/        # Synthetic applications used to pressure-test scheduling
└── viz/              # Empty for now; drop notebooks or plots here
```

## Requirements
- Linux kernel 6.12+ (or mainline with `CONFIG_SCHED_CLASS_EXT=y` and BTF enabled).
- `bpftool` built against the running kernel headers.
- LLVM/Clang (tested with 16+) for compiling BPF bytecode.
- `make`, `gcc`, and `pthread` headers for the workload binaries.
- Optional: a pacman-based distro if you plan to run the CachyOS helper script.

## Build and Load the SCX Scheduler
> The commands below assume you have sufficient privileges (root or sudo) and `bpftool` is new enough to understand `struct_ops` objects.

1. **Generate `vmlinux.h` if missing.**
	```sh
	cd scheduler
	sudo bpftool btf dump file /sys/kernel/btf/vmlinux format c > vmlinux.h
	```
	Skip this step if the header already exists for your running kernel.

2. **Compile the BPF object.**
	```sh
	make        # produces simple.bpf.o
	```

3. **Load and register the scheduler.**
	```sh
	sudo bpftool prog loadall simple.bpf.o /sys/fs/bpf/simple_scheduler type struct_ops
	sudo bpftool struct_ops register /sys/fs/bpf/simple_scheduler simple_ops
	```
	After registration you should see `simple_scheduler` appear in `sched_ext` traces (e.g., `sudo cat /sys/kernel/debug/sched/ext`).

4. **Unregister / cleanup.**
	```sh
	sudo bpftool struct_ops unregister /sys/fs/bpf/simple_scheduler simple_ops
	sudo rm -rf /sys/fs/bpf/simple_scheduler
	make clean
	```

## Workload Generators
Inside `workloads/` you will find three binaries; build them all with `make` and run whichever mix you need on arbitrary CPU counts.

| Binary | Behavior | Usage example |
| ------ | -------- | ------------- |
| `hog` | Tight `nop` spin-loop to fully occupy CPUs (no IO). | `./hog 8`
| `critical` | Atomic counter increment with optional periodic sleep to simulate IO waits. Prints throughput with precise timing. | `./critical 8 200` (8 threads, 200 µs sleep every ~1000 ops)
| `critical_2` | Per-thread cache-line-aligned counters with light computation to probe NUMA/cache behavior. | `./critical_2 16`

Terminate any workload with `Ctrl+C`; each binary traps `SIGINT` and drains threads before exiting.

## CachyOS Repository Helper
`scheduler/cachyos-repo/cachyos-repo.sh` is an upstream script (GPL-2.0+) that installs or removes the CachyOS pacman repositories, including tuned kernels such as the SCX-enabled CachyOS builds. Run `sudo ./cachyos-repo.sh --install` on Arch/Artix-based systems to add the mirror lists; use `--remove` to roll back. Review the script before executing it and note that it rewrites `/etc/pacman.conf` while keeping a `.bak` backup.

## Visualization Sandbox
The `viz/` directory is intentionally empty. Drop notebooks, flamegraphs, or any analysis artifacts that help explain how a given scheduler variant behaved during a run. Keeping plots alongside the code that generated them makes regressions easier to spot.

## Troubleshooting
- **`bpftool` cannot register `simple_ops`:** Ensure the kernel was built with `CONFIG_DEBUG_INFO_BTF` and that the running kernel exposes `/sys/kernel/btf/vmlinux`.
- **`sched_ext` rejects the program as non-GPL:** The BPF object embeds `char _license[] = "GPL"` already; double-check that you did not strip sections when copying the artifact.
- **Workload binaries fail to link:** Install `build-essential` (Debian/Ubuntu) or the equivalent toolchain group for your distro; all workloads only depend on POSIX threads and libc.
