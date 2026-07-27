use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub type CpuId = u16;
pub type LlcId = u16;
pub type PartitionId = u16;
pub type TaskId = u32;
pub type EnqueueOpId = u64;

pub const PARTITION_A: PartitionId = 0;
pub const LLC0: LlcId = 0;
pub const LLC1: LlcId = 1;
pub const CPU0: CpuId = 0;
pub const CPU1: CpuId = 1;
pub const CPU2: CpuId = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cpu {
    pub id: CpuId,
    pub llc: LlcId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Task {
    pub id: TaskId,
    pub allowed_cpus: BTreeSet<CpuId>,
    pub progress: u64,
}

impl Task {
    pub fn new(id: TaskId, allowed_cpus: impl IntoIterator<Item = CpuId>) -> Self {
        Self {
            id,
            allowed_cpus: allowed_cpus.into_iter().collect(),
            progress: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnqueueTicket {
    pub op_id: EnqueueOpId,
    pub task_id: TaskId,
    pub target_llc: LlcId,
    pub observed_mask_generation: u64,
    pub observed_target_had_cpu: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolState {
    partition_id: PartitionId,
    topology: BTreeMap<CpuId, Cpu>,
    published_cpus: BTreeSet<CpuId>,
    mask_generation: u64,
    queues: BTreeMap<LlcId, VecDeque<TaskId>>,
    draining: BTreeSet<LlcId>,
    tasks: BTreeMap<TaskId, Task>,
    pending_enqueues: BTreeMap<EnqueueOpId, EnqueueTicket>,
    next_op_id: EnqueueOpId,
}

impl ProtocolState {
    pub fn example_topology() -> Self {
        let topology = [
            Cpu {
                id: CPU0,
                llc: LLC0,
            },
            Cpu {
                id: CPU1,
                llc: LLC1,
            },
            Cpu {
                id: CPU2,
                llc: LLC1,
            },
        ];

        Self::new(
            PARTITION_A,
            topology,
            [CPU0, CPU1],
            [Task::new(100, [CPU0, CPU1])],
        )
    }

    pub fn new(
        partition_id: PartitionId,
        topology: impl IntoIterator<Item = Cpu>,
        published_cpus: impl IntoIterator<Item = CpuId>,
        tasks: impl IntoIterator<Item = Task>,
    ) -> Self {
        let topology = topology
            .into_iter()
            .map(|cpu| (cpu.id, cpu))
            .collect::<BTreeMap<_, _>>();

        assert!(
            !topology.is_empty(),
            "ProtocolState requires at least one CPU"
        );

        let published_cpus = published_cpus.into_iter().collect::<BTreeSet<_>>();
        for cpu_id in &published_cpus {
            assert!(
                topology.contains_key(cpu_id),
                "published CPU must exist in topology"
            );
        }

        let mut queues = BTreeMap::new();
        for cpu in topology.values() {
            queues.entry(cpu.llc).or_insert_with(VecDeque::new);
        }

        let tasks = tasks
            .into_iter()
            .map(|task| {
                assert!(
                    !task.allowed_cpus.is_empty(),
                    "task must be eligible for at least one CPU"
                );
                for cpu_id in &task.allowed_cpus {
                    assert!(
                        topology.contains_key(cpu_id),
                        "task affinity CPU must exist in topology"
                    );
                }
                (task.id, task)
            })
            .collect();

        Self {
            partition_id,
            topology,
            published_cpus,
            mask_generation: 0,
            queues,
            draining: BTreeSet::new(),
            tasks,
            pending_enqueues: BTreeMap::new(),
            next_op_id: 1,
        }
    }

    pub fn partition_id(&self) -> PartitionId {
        self.partition_id
    }

    pub fn mask_generation(&self) -> u64 {
        self.mask_generation
    }

    pub fn task_progress(&self, task_id: TaskId) -> u64 {
        self.tasks.get(&task_id).map_or(0, |task| task.progress)
    }

    pub fn record_task_progress(&mut self, task_id: TaskId, delta: u64) {
        let task = self.tasks.get_mut(&task_id).expect("task must exist");
        task.progress += delta;
    }

    pub fn published_cpus(&self) -> Vec<CpuId> {
        self.published_cpus.iter().copied().collect()
    }

    pub fn task_allowed_cpus(&self, task_id: TaskId) -> Vec<CpuId> {
        self.tasks
            .get(&task_id)
            .map(|task| task.allowed_cpus.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn llc_for_cpu(&self, cpu_id: CpuId) -> Option<LlcId> {
        self.topology.get(&cpu_id).map(|cpu| cpu.llc)
    }

    pub fn has_cpu_in_llc(&self, llc: LlcId) -> bool {
        self.published_cpus
            .iter()
            .any(|cpu_id| self.topology.get(cpu_id).is_some_and(|cpu| cpu.llc == llc))
    }

    pub fn queue_len(&self, llc: LlcId) -> usize {
        self.queues.get(&llc).map_or(0, VecDeque::len)
    }

    pub fn drain_enabled(&self, llc: LlcId) -> bool {
        self.draining.contains(&llc)
    }

    pub fn pending_enqueue_count(&self) -> usize {
        self.pending_enqueues.len()
    }

    pub fn queued_tasks(&self, llc: LlcId) -> Vec<TaskId> {
        self.queues
            .get(&llc)
            .map(|queue| queue.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn enqueue_select(&mut self, task_id: TaskId, preferred_llc: LlcId) -> EnqueueTicket {
        assert!(
            self.tasks.contains_key(&task_id),
            "enqueue_select requires a known task"
        );

        let target_llc = if self.has_cpu_in_llc(preferred_llc) {
            preferred_llc
        } else {
            self.first_eligible_published_llc(task_id)
                .unwrap_or(preferred_llc)
        };

        let ticket = EnqueueTicket {
            op_id: self.next_op_id,
            task_id,
            target_llc,
            observed_mask_generation: self.mask_generation,
            observed_target_had_cpu: self.has_cpu_in_llc(target_llc),
        };

        self.next_op_id += 1;
        self.pending_enqueues.insert(ticket.op_id, ticket.clone());
        ticket
    }

    pub fn publish_mask(&mut self, new_cpus: impl IntoIterator<Item = CpuId>) {
        let new_cpus = new_cpus.into_iter().collect::<BTreeSet<_>>();
        for cpu_id in &new_cpus {
            assert!(
                self.topology.contains_key(cpu_id),
                "publish_mask requires known CPUs"
            );
        }

        self.published_cpus = new_cpus;
        self.mask_generation += 1;
    }

    pub fn update_observe_queue(&mut self, llc: LlcId) {
        if !self.has_cpu_in_llc(llc) && self.queue_len(llc) > 0 {
            self.draining.insert(llc);
        }
    }

    pub fn enqueue_commit(&mut self, ticket: EnqueueTicket) {
        let pending = self
            .pending_enqueues
            .remove(&ticket.op_id)
            .expect("enqueue_commit requires a pending ticket");

        assert_eq!(
            pending, ticket,
            "enqueue ticket was modified after selection"
        );
        self.queues
            .entry(ticket.target_llc)
            .or_default()
            .push_back(ticket.task_id);
    }

    pub fn invalid_stable_stranding(&self, llc: LlcId) -> bool {
        self.pending_enqueues.is_empty()
            && self.queue_len(llc) > 0
            && !self.has_cpu_in_llc(llc)
            && !self.drain_enabled(llc)
            && self
                .queued_tasks(llc)
                .iter()
                .all(|task_id| self.has_eligible_cpu_outside_llc(*task_id, llc))
    }

    pub fn has_eligible_cpu_outside_llc(&self, task_id: TaskId, llc: LlcId) -> bool {
        let task = self.tasks.get(&task_id).expect("task must exist");
        self.published_cpus.iter().any(|cpu_id| {
            task.allowed_cpus.contains(cpu_id)
                && self.topology.get(cpu_id).is_some_and(|cpu| cpu.llc != llc)
        })
    }

    pub fn force_drain_for_test_recovery(&mut self, llc: LlcId) -> Vec<TaskId> {
        self.draining.insert(llc);
        self.queues
            .entry(llc)
            .or_default()
            .drain(..)
            .collect::<Vec<_>>()
    }

    fn first_eligible_published_llc(&self, task_id: TaskId) -> Option<LlcId> {
        let task = self.tasks.get(&task_id)?;
        self.published_cpus.iter().find_map(|cpu_id| {
            if task.allowed_cpus.contains(cpu_id) {
                self.topology.get(cpu_id).map(|cpu| cpu.llc)
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn race_reaches_invalid_stable_state() {
        let mut state = ProtocolState::example_topology();

        let ticket = state.enqueue_select(100, LLC0);
        state.publish_mask([CPU1]);
        state.update_observe_queue(LLC0);
        state.enqueue_commit(ticket);

        assert!(state.invalid_stable_stranding(LLC0));
        assert_eq!(state.queue_len(LLC0), 1);
        assert!(!state.has_cpu_in_llc(LLC0));
        assert!(!state.drain_enabled(LLC0));
        assert!(state.has_eligible_cpu_outside_llc(100, LLC0));
    }

    #[test]
    fn enqueue_then_update_enables_drain() {
        let mut state = ProtocolState::example_topology();

        let ticket = state.enqueue_select(100, LLC0);
        state.enqueue_commit(ticket);
        state.publish_mask([CPU1]);
        state.update_observe_queue(LLC0);

        assert!(!state.invalid_stable_stranding(LLC0));
        assert!(state.drain_enabled(LLC0));
    }

    #[test]
    fn update_then_enqueue_avoids_orphan_llc() {
        let mut state = ProtocolState::example_topology();

        state.publish_mask([CPU1]);
        state.update_observe_queue(LLC0);
        let ticket = state.enqueue_select(100, LLC0);
        assert_eq!(ticket.target_llc, LLC1);
        state.enqueue_commit(ticket);

        assert_eq!(state.queue_len(LLC0), 0);
        assert_eq!(state.queue_len(LLC1), 1);
        assert!(!state.invalid_stable_stranding(LLC0));
    }

    #[test]
    fn transient_state_before_commit_is_not_a_stable_failure() {
        let mut state = ProtocolState::example_topology();

        let _ticket = state.enqueue_select(100, LLC0);
        state.publish_mask([CPU1]);
        state.update_observe_queue(LLC0);

        assert_eq!(state.pending_enqueue_count(), 1);
        assert!(!state.invalid_stable_stranding(LLC0));
    }

    #[test]
    fn recovery_requires_task_to_remain_eligible_elsewhere() {
        let mut state = ProtocolState::example_topology();
        let ticket = state.enqueue_select(100, LLC0);
        state.publish_mask([CPU1]);
        state.update_observe_queue(LLC0);
        state.enqueue_commit(ticket);

        assert!(state.has_eligible_cpu_outside_llc(100, LLC0));
        let drained = state.force_drain_for_test_recovery(LLC0);
        assert_eq!(drained, vec![100]);
        assert_eq!(state.queue_len(LLC0), 0);
    }
}
