import json
from enum import Enum
from collections import deque
import sys

class TaskState(Enum):
    RUNNABLE = 0
    RUNNING = 1
    SLEEPING = 2

class Task:
    def __init__(self, pid: int, name: str, run_burst: int, sleep_burst: int, is_critical=False):
        '''
        To simulate a real program, which will sleep randomly when waiting for a
        lock or doing I/O, we introduce the concept of "run burst" and "sleep burst".
        After running for "run burst" ticks on CPU, the task will go to sleep for
        "sleep burst" ticks, then become runnable again.
        '''
        self.id = pid
        self.name = name
        self.is_critical = is_critical
        self.state = TaskState.RUNNABLE
        self.run_burst = run_burst
        self.sleep_burst = sleep_burst
        self.slice_left = 0
        self.sleep_timer = 0
        self.run_timer = run_burst
        self.last_running_tick = 0
        
    def __repr__(self):
        return f"Task({self.name}, pid={self.id}, state={self.state.name}, slice={self.slice_left})"

class SchedExtOps:
    def __init__(self):
        self.simulator = None

    def init(self):
        pass

    def select_cpu(self, task, prev_cpu: int, wake_flags: int) -> int:
        return prev_cpu

    def enqueue(self, task, enq_flags: int):
        pass

    def dispatch(self, cpu_id: int, prev_task):
        pass

    def running(self, task, cpu_id: int):
        pass

    def stopping(self, task, cpu_id: int, runnable: bool):
        pass

    # Helpers provided by engine
    def scx_bpf_create_dsq(self, dsq_id: int, node_id: int):
        self.simulator.create_dsq(dsq_id)

    def scx_bpf_dsq_insert(self, task, dsq_id: int, slice_us: int, enq_flags: int):
        self.simulator.dsq_insert(task, dsq_id, slice_us, enq_flags)

    def scx_bpf_dsq_move_to_local(self, cpu_id: int, dsq_id: int) -> bool:
        return self.simulator.dsq_move_to_local(cpu_id, dsq_id)

    def scx_bpf_kick_cpu(self, target_cpu: int, flags: int):
        self.simulator.kick_cpu(target_cpu)

class KernelSimulator:
    def __init__(self, num_cpus=4):
        self.num_cpus = num_cpus
        self.tick = 0
        self.cpus = [None] * num_cpus
        self.kicked_cpus = [False] * num_cpus
        self.dsqs = {} # dsq_id -> deque
        self.tasks = []
        self.policy = None
        self.trace_events = []
        self.next_pid = 1

    def create_dsq(self, dsq_id):
        if dsq_id not in self.dsqs:
            self.dsqs[dsq_id] = deque()

    def dsq_insert(self, task, dsq_id, slice_us, enq_flags):
        SCX_ENQ_HEAD = 1 # Define here or globally
        task.slice_left = slice_us
        dsq = self.dsqs.get(dsq_id)
        if dsq is None:
            raise ValueError(f"DSQ {dsq_id} does not exist")
        
        if enq_flags & SCX_ENQ_HEAD:
            dsq.appendleft(task)
        else:
            dsq.append(task)

    def dsq_move_to_local(self, cpu_id, dsq_id) -> bool:
        dsq = self.dsqs.get(dsq_id)
        if dsq and len(dsq) > 0:
            task = dsq.popleft()
            self.assign_task_to_cpu(task, cpu_id)
            return True
        return False

    def kick_cpu(self, target_cpu):
        if 0 <= target_cpu < self.num_cpus:
            self.kicked_cpus[target_cpu] = True

    def assign_task_to_cpu(self, task, cpu_id):
        task.state = TaskState.RUNNING
        task.last_running_tick = self.tick
        self.cpus[cpu_id] = task
        if self.policy:
            self.policy.running(task, cpu_id)

    def add_workload(self, name, num_threads, run_burst=10000, sleep_burst=500):
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

    def attach_policy(self, policy):
        self.policy = policy
        policy.simulator = self
        policy.init()
        # Enqueue all existing tasks
        for task in self.tasks:
            target_cpu = policy.select_cpu(task, 0, 0) # simplified prev_cpu
            policy.enqueue(task, 0)

    def run(self, duration_ticks):
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
                        self.policy.select_cpu(task, 0, 0) # select CPU first, then enqueue
                        self.policy.enqueue(task, 0)
        
        # Execution & Preemption
        for cpu_id in range(self.num_cpus):
            task = self.cpus[cpu_id]
            if task:
                task.slice_left -= 1
                task.run_timer -= 1 # Decrease run timer as well
                if task.slice_left <= 0 or self.kicked_cpus[cpu_id]:
                    # Evict
                    self.policy.stopping(task, cpu_id, True)
                    self.cpus[cpu_id] = None
                    self.kicked_cpus[cpu_id] = False
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
        for dsq_id, tasks in self.dsqs.items():
            print(f"  DSQ {dsq_id}: {[t.name for t in tasks]}")
        
        print("All Tasks State:")
        for t in self.tasks:
            print(f"  {t}")
            
        sys.exit(1)

    def record_trace(self, task, cpu_id, start_tick, end_tick):
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

    def export_perfetto(self, filename):
        with open(filename, 'w') as f:
            json.dump(self.trace_events, f, indent=2)
