# sched-ext Verus

This project explores how to verify a real sched-ext scheduler with Verus.

The scheduler logic should be written once in Rust and call a small sched-ext API. The same logic will use:

- a real backend when loaded as a scheduler;
- a mock/spec backend during testing and verification.

Linux and sched-ext kernel APIs are treated as trusted components. Verus verifies the scheduler logic and its interactions with the specified API.

The mock, Verus, and BPF object builds pass locally. Kernel attachment remains
to be tested on a sched-ext-enabled host; the current WSL kernel does not expose
sched-ext.

## Stages

1. Write and load a minimal Rust scheduler.
2. Implement a mock backend for the same API.
3. Verify the shared scheduler logic with Verus.
4. Extend the framework to more complex schedulers.

## Checks

```sh
cargo test
verus --crate-type=lib --cfg 'feature="std"' --cfg 'feature="mock"' src/lib.rs
```

See `DESIGN.md` for the boundary and `ATTACH.md` for the sched-ext host test.
