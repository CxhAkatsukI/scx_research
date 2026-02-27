import json
from enum import Enum
from collections import deque
import random
import sys

class TaskState(Enum):
    RUNNABLE = 0
    RUNNING = 1
    SLEEPING = 2
    
class ScxBuiltinDsqId(Enum):
	SCX_DSQ_FLAG_BUILTIN = 9223372036854775808
	SCX_DSQ_FLAG_LOCAL_ON = 4611686018427387904
	SCX_DSQ_INVALID = 9223372036854775808
	SCX_DSQ_GLOBAL = 9223372036854775809
	SCX_DSQ_LOCAL = 9223372036854775810
	SCX_DSQ_LOCAL_ON = 13835058055282163712
	SCX_DSQ_LOCAL_CPU_MASK = 4294967295

class Task:
    def __init__(self, pid: int, name: str, run_burst: int, sleep_burst: int, is_critical=False):
        '''
        To simulate a real program, which will sleep randomly when waiting for a
        lock or doing I/O, we introduce the concept of "run burst" and "sleep burst".
        After running for "run burst" ticks on CPU, the task will go to sleep for
        "sleep burst" ticks, then become runnable again.
        '''
        self.id: int = pid
        self.name: str = name
        self.is_critical: bool = is_critical
        self.state: TaskState = TaskState.RUNNABLE
        self.run_burst: int = run_burst
        self.sleep_burst: int = sleep_burst
        self.slice_left: int = 0
        self.sleep_timer: int = 0
        self.run_timer: int = run_burst
        self.enqueue_tick: int = 0 # track the moment of enqueueing
        self.last_running_tick: int = 0
        self.target_cpu: int = 0 # keep track of which CPU the task is targeting for local DSQ insertion
        
    def __repr__(self):
        return f"Task({self.name}, pid={self.id}, state={self.state.name}, slice={self.slice_left})"

class SchedExtOps:
    def __init__(self):
        self.simulator: KernelSimulator = None

    def init(self):
        pass

    def select_cpu(self, task: Task, prev_cpu: int, wake_flags: int) -> int:
        return prev_cpu

    def enqueue(self, task, enq_flags: int):
        pass

    def dispatch(self, cpu_id: int, prev_task):
        pass

    def running(self, task: Task, cpu_id: int):
        pass

    def stopping(self, task: Task, cpu_id: int, runnable: bool):
        pass

    # Helpers provided by engine
    def scx_bpf_create_dsq(self, dsq_id: int, node_id: int):
        self.simulator.create_dsq(dsq_id)

    def scx_bpf_dsq_insert(self, task, dsq_id: int, slice_us: int, enq_flags: int):
        self.simulator.dsq_insert(task, dsq_id, slice_us, enq_flags)

    def scx_bpf_dsq_move_to_local(self, cpu_id: int, dsq_id: int) -> bool:
        return self.simulator.dsq_move_to_local(cpu_id, dsq_id)

    def scx_bpf_kick_cpu(self, target_cpu: int, flags: int):
        self.simulator.kick_cpu(target_cpu, flags)

class KernelSimulator:
    def __init__(self, num_cpus=4):
        self.num_cpus: int = num_cpus
        self.tick: int = 0
        self.cpus: list[Task] = [None] * num_cpus
        self.kicked_cpus: list[int] = [0] * num_cpus # Support kick latency
        self.global_dsqs: dict[int, deque] = {} # dsq_id -> deque
        self.local_dsqs: dict[int, deque] = {} # cpu_id -> deque
        self.tasks: list[Task] = []
        self.policy: SchedExtOps = None
        self.trace_events: list[dict] = []
        self.next_pid: int = 1
        # create default local DSQs for each CPU
        for cpu_ids in range(num_cpus):
            if cpu_ids not in self.local_dsqs:
                self.local_dsqs[cpu_ids] = deque()

    # create global DSQs on demand when policy calls scx_bpf_create_dsq
    def create_dsq(self, dsq_id: int):
        if dsq_id not in self.global_dsqs:
            self.global_dsqs[dsq_id] = deque()

    def dsq_insert(self, task: Task, dsq_id: int, slice_us: int, enq_flags: int):
        SCX_ENQ_HEAD = 1 # Define here or globally
        task.slice_left = slice_us
        task.enqueue_tick = self.tick

        if dsq_id == ScxBuiltinDsqId.SCX_DSQ_LOCAL.value:
            # Simplified logic for a simulator
            dsq = self.local_dsqs.get(task.target_cpu)
        else:
            dsq = self.global_dsqs.get(dsq_id)

        if dsq is None:
            raise ValueError(f"DSQ {dsq_id} does not exist")
        
        # whether the task is inserted at the head
        if enq_flags & SCX_ENQ_HEAD:
            dsq.appendleft(task)
        else:
            dsq.append(task)

    def dsq_move_to_local(self, cpu_id: int, dsq_id: int) -> bool:
        dsq = self.global_dsqs.get(dsq_id)
        if dsq and len(dsq) > 0:
            task = dsq.popleft()
            self.assign_task_to_cpu(task, cpu_id)
            return True
        return False

    def kick_cpu(self, target_cpu: int, flags: int):
        if 0 <= target_cpu < self.num_cpus:
            self.kicked_cpus[target_cpu] = random.randint(0, 5) # Simulate a kick latency of 1-3 ticks

    def assign_task_to_cpu(self, task: Task, cpu_id: int):
        # Record waiting time in DSQ before running
        wait_duration = self.tick - task.enqueue_tick # Calculate waiting time in the DSQ
        self.record_trace_raw(
            f"WAIT: {task.name}",
            cpu_id,
            task.enqueue_tick,
            self.tick
        )
        
        # Record the running event
        task.state = TaskState.RUNNING
        task.last_running_tick = self.tick
        self.record_trace(
            task,
            cpu_id,
            task.last_running_tick,
            task.last_running_tick + task.slice_left
        )

        self.cpus[cpu_id] = task
        if self.policy:
            self.policy.running(task, cpu_id)

    def add_workload(self, name: str, num_threads: int, run_burst=10000, sleep_burst=0):
        is_critical = (name == "critical")
        for _ in range(num_threads):
            task = Task(self.next_pid, f"{name}_{self.next_pid-1}", run_burst, sleep_burst, is_critical)
            self.next_pid += 1
            self.tasks.append(task)
            # Initial enqueue
            if self.policy:
                # This might be tricky if policy isn't attached yet. 
                # main.py suggests add_workload happens BEFORE attach_policy.
                pass

    def attach_policy(self, policy: SchedExtOps):
        self.policy = policy
        policy.simulator = self
        policy.init()
        # Enqueue all existing tasks
        for task in self.tasks:
            task.target_cpu = policy.select_cpu(task, 0, 0) # simplified prev_cpu
            policy.enqueue(task, 0)

    def run(self, duration_ticks: int):
        for _ in range(duration_ticks):
            self.step()

    def step(self):
        self.tick += 1
        
        # Processing Sleeping Tasks
        for task in self.tasks:
            if task.state == TaskState.SLEEPING:
                task.sleep_timer -= 1
                if task.sleep_timer <= 0:
                    task.state = TaskState.RUNNABLE
                    task.run_timer = task.run_burst # Reset run timer for the new cycle
                    if self.policy:
                        task.target_cpu = self.policy.select_cpu(task, 0, 0) # select CPU first, then enqueue
                        self.policy.enqueue(task, 0)
        
        # Execution & Preemption
        for cpu_id in range(self.num_cpus):
            task = self.cpus[cpu_id]
            
            # Check if a kick is pending
            kick_triggered = False
            if self.kicked_cpus[cpu_id] > 0:
                self.kicked_cpus[cpu_id] -= 1
                if self.kicked_cpus[cpu_id] == 0:
                    kick_triggered = True

            if task:
                task.slice_left -= 1 # Decrease slice left
                task.run_timer -= 1 # Decrease run timer as well
                if task.slice_left <= 0 or kick_triggered:
                    # Evict
                    self.policy.stopping(task, cpu_id, True)
                    self.cpus[cpu_id] = None
                    self.kicked_cpus[cpu_id] = 0
                    # Decide next state of the task
                    if task.run_timer > 0:
                        task.state = TaskState.RUNNABLE
                        self.policy.enqueue(task, 0)
                    else:
                        # Task has exhausted its run burst, go to sleep
                        task.state = TaskState.SLEEPING
                        task.sleep_timer = task.sleep_burst

        # Dispatching
        for cpu_id in range(self.num_cpus):
            if self.cpus[cpu_id] is None:
                if self.local_dsqs[cpu_id]:
                    next_task = self.local_dsqs[cpu_id].popleft()
                    self.assign_task_to_cpu(next_task, cpu_id)
                else:
                    self.policy.dispatch(cpu_id, None)

        # Assertion Check
        # Skip for now to get the trace file
        # self.check_priority_inversion()

    def check_priority_inversion(self):
        critical_waiting = any(t.is_critical and t.state == TaskState.RUNNABLE for t in self.tasks)
        if critical_waiting:
            # Check if CPU 0 or 1 is running a hog task
            for cpu_id in [0, 1]:
                if cpu_id < self.num_cpus:
                    running_task = self.cpus[cpu_id]
                    if running_task and not running_task.is_critical: # "hog" tasks are not critical
                        self.dump_state_and_exit()

    def dump_state_and_exit(self):
        print(f"[ASSERTION FAILED] Priority Inversion Detected!")
        print(f"Current Tick: {self.tick}")
        print("State of all CPUs:")
        for i, task in enumerate(self.cpus):
            if task:
                print(f"  CPU {i}: {task.name} (PID {task.id}), slice_left: {task.slice_left}")
            else:
                print(f"  CPU {i}: IDLE")
        
        print("State of all DSQs:")
        for dsq_id, tasks in self.global_dsqs.items():
            print(f"GLOBAL  DSQ {dsq_id}: {[t.name for t in tasks]}")
        for dsq_id, tasks in self.local_dsqs.items():
            print(f"LOCAL  DSQ {dsq_id}: {[t.name for t in tasks]}")
        
        print("All Tasks State:")
        for t in self.tasks:
            print(f"  {t}")
            
        sys.exit(1)

    def record_trace(self, task: Task, cpu_id: int, start_tick: int, end_tick: int):
        duration = end_tick - start_tick
        if duration > 0:
            self.trace_events.append({
                "name": task.name,
                "cat": "sched",
                "ph": "X",
                "ts": start_tick,
                "dur": duration,
                "pid": 0,
                "tid": f"CPU {cpu_id}"
            })
            
    def record_trace_raw(self, name: str, cpu_id: int, start_tick: int, end_tick: int):
        """
        A more generic trace recording function that allows custom event names.
        This can be used for recording events that are not directly tied to task
        execution, like waiting in DSQ, being kicked, etc.
        """
        duration = end_tick - start_tick
        if duration > 0:
            self.trace_events.append({
                "name": name,
                "cat": "sched",
                "ph": "X",
                "ts": start_tick,
                "dur": duration,
                "pid": 0,
                "tid": f"CPU {cpu_id}"
            })


    def export_perfetto(self, filename: str):
        with open(filename, 'w') as f:
            json.dump(self.trace_events, f, indent=2)
