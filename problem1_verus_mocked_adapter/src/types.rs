#![allow(dead_code)]

pub type CpuId = u16;
pub type LlcId = u16;
pub type EnqueueSeq = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunMode {
    Report,
    Deterministic,
    Stochastic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Topology {
    pub target_cpu: CpuId,
    pub target_llc: LlcId,
    pub recovery_cpu: CpuId,
    pub recovery_llc: LlcId,
    pub control_cpu: CpuId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskRef {
    pub pid: i32,
    pub enqueue_seq: EnqueueSeq,
}

impl TaskRef {
    pub fn new(pid: i32, enqueue_seq: EnqueueSeq) -> Self {
        Self { pid, enqueue_seq }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub mask_generation: u64,
    pub q: usize,
    pub c: bool,
    pub d: bool,
    pub selected_once: bool,
    pub recovered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchTarget {
    Any,
    TargetCpu,
    RecoveryCpu,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dispatch {
    pub task: TaskRef,
    pub target: DispatchTarget,
    pub snapshot: Snapshot,
}
