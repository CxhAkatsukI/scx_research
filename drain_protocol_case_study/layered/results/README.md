# Layered stall validation evidence

These are canonical copies of the two successful CachyOS runs. Bulk VM builds,
failed attempts, and other `.vm-run` data remain untracked.

## Default anti-stall: `stall-default/`

- Target PID 41750, DSQ 5.
- Causal match: `Q=1, C=0, D=0, on_dsq=1`.
- 248 samples observed state `R`; `/proc` CPU ticks stayed at zero.
- Enqueue-to-running delay: 12.847294 seconds.
- Default anti-stall eventually rescued the target.

## Anti-stall disabled: `stall-watchdog/`

- Target PID 45841, DSQ 7.
- Causal sequence in kernel nanoseconds:

  ```text
  request        10781357319363
  clear          10781358656580
  empty recheck  10781358656658
  kernel landing 10781359035198
  first running  10825082182334
  ```

- Landing observation: `Q=1, C=0, D=0, on_dsq=1`.
- 842 samples observed state `R`; `/proc` CPU ticks stayed at one.
- The kernel reported `runnable task stall` after 43.722 seconds and activated
  sched_ext bypass. The first-running event followed 43.723147 seconds after
  landing and corresponds to watchdog recovery, not normal layered dispatch.
- `P1_LOST 0`; no probe records were dropped.

The preserved `run.log` was produced before the final result-classifier update,
so its loader status 1 and earlier `INCONCLUSIVE` line describe the old wrapper's
handling of the watchdog exit. The current `vm-stall.sh` recognizes a causally
matched watchdog stall as a successful reproduction. `monitor.json`,
`monitor.log`, and `trace.log` contain the underlying evidence.

## Interpretation

The probes bracket Q with C/D reads but do not obtain one atomic snapshot and
can affect timing. The experiment intentionally freezes userspace CPU-mask
updates after a match and uses a high ordinary cross-LLC migration threshold.
The watchdog and the absence of target CPU progress provide independent
task-level evidence. These runs do not prove permanent starvation with default
anti-stall.
