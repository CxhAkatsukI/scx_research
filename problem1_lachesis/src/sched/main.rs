// SPDX-License-Identifier: GPL-2.0
#![no_std]
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
use p1_runtime::*;

verus! {
pub struct DrainScheduler { pub shared: Shared }
impl Policy for DrainScheduler {
    fn select_cpu(&self, _p: Task, prev: i32, _flags: u64, _log: &mut Log) -> i32 { prev }

    fn enqueue(&self, p: Task, flags: u64, log: &mut Log) {
        if let Some(mut t) = ticket(&p, true) {
            if self.shared.finish_removed(&mut t, log) {
                if self.shared.update(REGISTER, false, log) {
                    // This store is NOT atomic with the preceding successful CAS.
                    t.set_owned(true, log);
                    insert(&p, TARGET, flags, log); // A request, not an immediate landing.
                    return;
                }
            }
        }
        insert(&p, GLOBAL, flags, log);
    }

    fn dispatch(&self, cpu: i32, _prev: Option<Task>, log: &mut Log) {
        let w = self.shared.packed.snapshot();
        if (cpu == 0 && w & 1 != 0) || w & 2 != 0 {
            let _moved = move_to_local(TARGET, log);
            // A successful move does not identify a task and does not release N.
            self.shared.update(RETIRE, false, log);
        }
    }

    fn running(&self, p: Task, log: &mut Log) {
        if let Some(mut t) = ticket(&p, false) { self.shared.finish_removed(&mut t, log); }
    }
    fn disable(&self, p: Task, log: &mut Log) {
        if let Some(mut t) = ticket(&p, false) { self.shared.finish_removed(&mut t, log); }
    }
    fn exit_task(&self, p: Task, _args: u64, log: &mut Log) {
        if let Some(mut t) = ticket(&p, false) { self.shared.finish_removed(&mut t, log); }
    }
    fn init(&self, _log: &mut Log) -> i32 { create_dsq(TARGET) }
}
pub fn config(s: &DrainScheduler, covered: bool, log: &mut Log) -> (r: i32)
    requires old(log).wf(), old(log).position() == Pc::Start,
    ensures final(log).wf(), final(log).position() == Pc::Start, r == 0 || r == -11,
{ if s.shared.update(PUBLISH, covered, log) { 0 } else { -11 } }
}

scheduler! {
    map: p1_ops,
    name: "p1_lachesis",
    flags: 1u64 << 3, // SCX_OPS_SWITCH_PARTIAL
    policy: P1: DrainScheduler = DrainScheduler { shared: Shared { packed: AtomicWord::new(1) } },
    ops {
        select_cpu as p1_select_cpu,
        enqueue as p1_enqueue,
        dispatch as p1_dispatch,
        running as p1_running,
        disable as p1_disable,
        exit_task as p1_exit_task,
    }
    sleepable { init as p1_init }
}

config_entry!(p1_config, P1, config);
