# Harness Boundary

This directory will contain workload drivers, mask-update orchestration,
deterministic gates, stochastic stress, recovery, and health checks.

The first real-load milestone must prove that:

- `/sys/kernel/sched_ext/state` becomes enabled during the experiment;
- the invalid state is observed only after the enqueue and mask update both
  return;
- the workload can be recovered onto an eligible CPU;
- sched-ext is disabled after cleanup;
- SSH/system health survives the run.

No root or kernel-loading commands are included in the initial protocol commit.
