# Adapter Boundary

This directory will contain the thin sched-ext adapter for the Problem 1
playground.

The adapter must:

- load only through the controlled harness;
- use partial switching for the experiment workload;
- expose real adapter/BPF state for `Q`, `C`, and `D`;
- forward protocol decisions instead of reimplementing scheduler semantics;
- unload cleanly after every run.

No real adapter code is present in the initial protocol commit.
