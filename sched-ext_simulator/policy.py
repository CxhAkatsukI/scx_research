from scx_sim import SchedExtOps

class MyPolicy(SchedExtOps):
    DSQ_NORMAL = 0
    DSQ_VIP = 1
    DSQ_HOG = 2
    
    SLICE_VIP = 50000
    SLICE_NORMAL = 10000
    SLICE_HOG = 1000
    
    SCX_ENQ_HEAD = 1

    def init(self):
        self.scx_bpf_create_dsq(self.DSQ_NORMAL, 0)
        self.scx_bpf_create_dsq(self.DSQ_VIP, 0)
        self.scx_bpf_create_dsq(self.DSQ_HOG, 0)

    def select_cpu(self, task, prev_cpu: int, wake_flags: int) -> int:
        if task.is_critical:
            target_cpu = prev_cpu if prev_cpu in [0, 1] else 0
            self.scx_bpf_kick_cpu(target_cpu, 0)
            return target_cpu
        return prev_cpu

    def enqueue(self, task, enq_flags: int):
        if task.is_critical:
            self.scx_bpf_dsq_insert(task, self.DSQ_VIP, self.SLICE_VIP, self.SCX_ENQ_HEAD)
        else:
            # Assuming non-critical tasks are "hog" for now based on the prompt's target policy description
            # "If hog, insert to DSQ_HOG with slice=1000, flag=0"
            self.scx_bpf_dsq_insert(task, self.DSQ_HOG, self.SLICE_HOG, 0)

    def dispatch(self, cpu_id: int, prev_task):
        if cpu_id in [0, 1]:
            if self.scx_bpf_dsq_move_to_local(cpu_id, self.DSQ_VIP):
                return
            if self.scx_bpf_dsq_move_to_local(cpu_id, self.DSQ_NORMAL):
                return
            if self.scx_bpf_dsq_move_to_local(cpu_id, self.DSQ_HOG):
                return
        elif cpu_id in [2, 3]:
            if self.scx_bpf_dsq_move_to_local(cpu_id, self.DSQ_NORMAL):
                return
            if self.scx_bpf_dsq_move_to_local(cpu_id, self.DSQ_HOG):
                return

    def running(self, task, cpu_id: int):
        # The engine handles task.last_running_tick
        pass

    def stopping(self, task, cpu_id: int, runnable: bool):
        self.simulator.record_trace(task, cpu_id, task.last_running_tick, self.simulator.tick)
