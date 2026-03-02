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
        self.vtime: int = 0 # virtual time for priority aging
        self.target_cpu: int = 0 # keep track of which CPU the task is targeting for local DSQ insertion
        
    def __repr__(self):
        return f"Task({self.name}, pid={self.id}, state={self.state.name}, slice={self.slice_left}, vtime={self.vtime})"

class SchedExtOps:
    def __init__(self):
        self.simulator: KernelSimulator = None
        self.current_cpu: int = 0 # Hardcode it into 0

    def init(self):
        pass

    def exit(self):
        pass

    def init_task(self, task: Task):
        pass

    def enable(self, task: Task):
        pass

    def select_cpu(self, task: Task, prev_cpu: int, wake_flags: int) -> int:
        return prev_cpu

    def enqueue(self, task, enq_flags: int):
        pass

    def dispatch(self, cpu_id: int, prev_task):
        pass

    def runnable(self, task: Task, enq_flags: int):
        pass

    def running(self, task: Task, cpu_id: int):
        pass

    def stopping(self, task: Task, cpu_id: int, runnable: bool):
        pass

    # --- SCX Core Functions ---
    def scx_bpf_create_dsq(self, dsq_id: int, node_id: int = -1):
        self.simulator.create_dsq(dsq_id)

    def scx_bpf_dsq_insert(self, task, dsq_id: int, slice_us: int, enq_flags: int):
        # Handle SCX_DSQ_LOCAL based on current execution context
        target_dsq = dsq_id
        if dsq_id == ScxBuiltinDsqId.SCX_DSQ_LOCAL.value:
            if self.current_cpu >= 0:
                target_dsq = ScxBuiltinDsqId.SCX_DSQ_FLAG_LOCAL_ON.value | self.current_cpu
            else:
                # If no CPU context (e.g. global init), fallback to global
                target_dsq = ScxBuiltinDsqId.SCX_DSQ_GLOBAL.value
        self.simulator.dsq_insert(task, target_dsq, slice_us, enq_flags)

    def scx_bpf_dsq_insert_vtime(self, task, dsq_id: int, slice_us: int, vtime: int, enq_flags: int):
        target_dsq = dsq_id
        if dsq_id == ScxBuiltinDsqId.SCX_DSQ_LOCAL.value:
            if self.current_cpu >= 0:
                target_dsq = ScxBuiltinDsqId.SCX_DSQ_FLAG_LOCAL_ON.value | self.current_cpu
            else:
                target_dsq = ScxBuiltinDsqId.SCX_DSQ_GLOBAL.value
                
        self.simulator.dsq_insert(task, target_dsq, slice_us, enq_flags, vtime=vtime)

    def scx_bpf_dsq_move_to_local(self, cpu_id: int, dsq_id: int) -> bool:
        return self.simulator.dsq_move_to_local(cpu_id, dsq_id)

    def scx_bpf_kick_cpu(self, target_cpu: int, flags: int):
        self.simulator.kick_cpu(target_cpu, flags)

    def scx_bpf_now(self) -> int:
        return self.simulator.tick

    def scx_bpf_nr_cpu_ids(self) -> int:
        return self.simulator.num_cpus

    def scx_bpf_task_cpu(self, task: Task) -> int:
        return task.target_cpu

    def scx_bpf_task_running(self, task: Task) -> bool:
        return task.state == TaskState.RUNNING

    def scx_bpf_dsq_nr_queued(self, dsq_id: int) -> int:
        if dsq_id == ScxBuiltinDsqId.SCX_DSQ_LOCAL.value:
            if self.current_cpu < 0:
                return 0
            dsq = self.simulator.local_dsqs.get(self.current_cpu)
        elif dsq_id == ScxBuiltinDsqId.SCX_DSQ_GLOBAL.value:
            dsq = self.simulator.global_dsqs.get(ScxBuiltinDsqId.SCX_DSQ_GLOBAL.value)
        elif (dsq_id & ScxBuiltinDsqId.SCX_DSQ_FLAG_LOCAL_ON.value) == ScxBuiltinDsqId.SCX_DSQ_FLAG_LOCAL_ON.value:
            cpu_id = dsq_id & ScxBuiltinDsqId.SCX_DSQ_LOCAL_CPU_MASK.value
            dsq = self.simulator.local_dsqs.get(cpu_id)
        else:
            dsq = self.simulator.global_dsqs.get(dsq_id)
        
        return len(dsq) if dsq is not None else 0

    # Pick an idle CPU
    def scx_bpf_pick_idle_cpu(self, cpumask: set[int], flags: int) -> int:
        idle_intersect = self.simulator.idle_cpumask.intersection(cpumask)
        if idle_intersect:
            return min(idle_intersect)
        return -1

    # Pick the default CPU (Prev one)
    def scx_bpf_select_cpu_dfl(self, task: Task, prev_cpu: int, wake_flags: int, is_idle: bool) -> int:
        if is_idle:
            return prev_cpu
        idle_cpu = self.scx_bpf_pick_idle_cpu(self.scx_bpf_get_online_cpumask(), 0)
        return idle_cpu if idle_cpu != -1 else prev_cpu

    # Only search for provided mask, return the prev CPU for default behavior
    def scx_bpf_select_cpu_and(self, cpumask: set[int], prev_cpu: int, wake_flags: int, is_idle: bool) -> int:
        idle_cpu = self.scx_bpf_pick_idle_cpu(cpumask, 0)
        return idle_cpu if idle_cpu != -1 else prev_cpu

    # We assume all CPUs are online, so return the full mask
    def scx_bpf_get_online_cpumask(self) -> set[int]:
        return set(range(self.simulator.num_cpus))

    # We don't have smt in this simulator
    def scx_bpf_get_idle_smtmask(self) -> set[int]:
        return self.simulator.idle_cpumask.copy()

    # Test and clear the idle state of the CPU
    def scx_bpf_test_and_clear_cpu_idle(self, cpu: int) -> bool:
        if cpu in self.simulator.idle_cpumask:
            self.simulator.idle_cpumask.remove(cpu)
            return True
        return False

    def scx_bpf_put_cpumask(self, cpumask: set[int]):
        pass

    # Print an error message and exit the simulator
    def scx_bpf_error(self, msg: str):
        print(f"SCX ERROR: {msg}")
        sys.exit(1)

    def scx_bpf_dispatch_cancel(self):
        pass

    def scx_bpf_dispatch_nr_slots(self) -> int:
        return 64

    # --- BPF Helpers ---
    # We print the message to console for visibility, but in a real BPF this would go to the kernel log buffer
    def bpf_printk(self, fmt: str, *args):
        print(f"BPF_PRINTK: {fmt % args}")

    # Does the symbol exist in the kernel? In our simulator, we can just check if it's a method in SchedExtOps
    def bpf_ksym_exists(self, sym: str) -> bool:
        return hasattr(self, sym)

    def bpf_core_read(self, src, size: int, offset: int = 0):
        # Simplified: just returns the object if it's a direct read
        return src

    def bpf_task_from_pid(self, pid: int) -> Task:
        return self.simulator.tasks_by_pid.get(pid)

    def bpf_task_release(self, task: Task):
        pass

    def bpf_get_smp_processor_id(self) -> int:
        return self.current_cpu

    def bpf_map_lookup_elem(self, map_obj: dict, key) -> any:
        return map_obj.get(key)

    def bpf_rcu_read_lock(self):
        pass

    def bpf_rcu_read_unlock(self):
        pass

    def bpf_cpumask_first(self, cpumask: set[int]) -> int:
        return min(cpumask) if cpumask else -1

    def bpf_cpumask_test_cpu(self, cpu: int, cpumask: set[int]) -> bool:
        return cpu in cpumask

    def bpf_task_storage_get(self, map_obj: dict, task: Task, value: any, flags: int) -> any:
        # In a real BPF, this would use a task-local storage map.
        # Here we can just use a dictionary or a task attribute.
        storage_key = (id(task), id(map_obj))
        if storage_key not in self.simulator.task_storage:
            self.simulator.task_storage[storage_key] = value
        return self.simulator.task_storage[storage_key]

    def bpf_user_ringbuf_drain(self, rb, callback, ctx, flags):
        # rb is a list/deque acting as a ringbuffer
        count = 0
        while rb:
            item = rb.pop(0)
            callback(item, ctx)
            count += 1
        return count

    def bpf_for(self, start, end, callback):
        for i in range(start, end):
            callback(i)

class KernelSimulator:
    def __init__(self, num_cpus=4):
        self.num_cpus: int = num_cpus
        self.tick: int = 0
        self.cpus: list[Task] = [None] * num_cpus
        self.kicked_cpus: list[int] = [0] * num_cpus # Support kick latency
        self.global_dsqs: dict[int, list[Task]] = {} # dsq_id -> list (for vtime support)
        self.local_dsqs: dict[int, list[Task]] = {} # cpu_id -> list
        self.tasks: list[Task] = []
        self.tasks_by_pid: dict[int, Task] = {}
        self.idle_cpumask: set[int] = set(range(num_cpus))
        self.task_storage: dict[tuple, any] = {}
        self.policy: SchedExtOps = None
        self.trace_events: list[dict] = []
        self.next_pid: int = 1
        # create default local DSQs for each CPU
        for cpu_ids in range(num_cpus):
            if cpu_ids not in self.local_dsqs:
                self.local_dsqs[cpu_ids] = []

    # create global DSQs on demand when policy calls scx_bpf_create_dsq
    def create_dsq(self, dsq_id: int):
        if dsq_id not in self.global_dsqs:
            self.global_dsqs[dsq_id] = []

    def dsq_insert(self, task: Task, dsq_id: int, slice_us: int, enq_flags: int, vtime: int = 0):
        SCX_ENQ_HEAD = 1 
        task.slice_left = slice_us
        task.enqueue_tick = self.tick

        if dsq_id == ScxBuiltinDsqId.SCX_DSQ_LOCAL.value:
            dsq = self.local_dsqs.get(task.target_cpu)
        elif (dsq_id & ScxBuiltinDsqId.SCX_DSQ_FLAG_LOCAL_ON.value) == ScxBuiltinDsqId.SCX_DSQ_FLAG_LOCAL_ON.value:
            cpu_id = dsq_id & ScxBuiltinDsqId.SCX_DSQ_LOCAL_CPU_MASK.value
            dsq = self.local_dsqs.get(cpu_id)
        elif dsq_id == ScxBuiltinDsqId.SCX_DSQ_GLOBAL.value:
            # If SCX_DSQ_GLOBAL is used, we need to ensure it exists
            if dsq_id not in self.global_dsqs:
                self.create_dsq(dsq_id)
            dsq = self.global_dsqs.get(dsq_id)
        else:
            dsq = self.global_dsqs.get(dsq_id)

        if dsq is None:
            raise ValueError(f"DSQ {dsq_id} does not exist")
        
        if vtime != 0:
            task.vtime = vtime
            dsq.append(task)
            dsq.sort(key=lambda x: getattr(x, 'vtime', 0))
        elif enq_flags & SCX_ENQ_HEAD:
            dsq.insert(0, task)
        else:
            dsq.append(task)

    def dsq_move_to_local(self, cpu_id: int, dsq_id: int) -> bool:
        dsq = self.global_dsqs.get(dsq_id)
        if dsq and len(dsq) > 0:
            task = dsq.pop(0)
            self.assign_task_to_cpu(task, cpu_id)
            return True
        return False

    def kick_cpu(self, target_cpu: int, flags: int):
        if 0 <= target_cpu < self.num_cpus:
            self.kicked_cpus[target_cpu] = random.randint(1, 3) # Simulate a kick latency

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

        self.cpus[cpu_id] = task
        if cpu_id in self.idle_cpumask:
            self.idle_cpumask.remove(cpu_id)

        if self.policy:
            self.policy.current_cpu = cpu_id
            self.policy.running(task, cpu_id)

    def add_workload(self, name: str, num_threads: int, run_burst=10000, sleep_burst=0):
        is_critical = (name == "critical")
        for _ in range(num_threads):
            task = Task(self.next_pid, f"{name}_{self.next_pid-1}", run_burst, sleep_burst, is_critical)
            self.tasks_by_pid[self.next_pid] = task
            self.next_pid += 1
            self.tasks.append(task)
            if self.policy:
                self.policy.current_cpu = 0
                self.policy.init_task(task)
                self.policy.enable(task)
                task.target_cpu = self.policy.select_cpu(task, 0, 0)
                self.policy.enqueue(task, 0)

    def attach_policy(self, policy: SchedExtOps):
        self.policy = policy
        policy.simulator = self
        policy.init()
        # Enqueue all existing tasks
        for task in self.tasks:
            self.policy.current_cpu = 0 # Global context
            self.policy.init_task(task)
            self.policy.enable(task)
            task.target_cpu = policy.select_cpu(task, 0, 0)
            self.policy.runnable(task, 0)
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
                    task.run_timer = task.run_burst
                    if self.policy:
                        self.policy.current_cpu = 0 # Wake context
                        self.policy.runnable(task, 0)
                        task.target_cpu = self.policy.select_cpu(task, 0, 0)
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
                task.slice_left -= 1
                task.run_timer -= 1
                if task.slice_left <= 0 or task.run_timer <= 0 or kick_triggered:
                    # Evict
                    if self.policy:
                        self.policy.current_cpu = cpu_id
                        self.policy.stopping(task, cpu_id, True)
                    
                    self.cpus[cpu_id] = None
                    self.idle_cpumask.add(cpu_id)
                    self.kicked_cpus[cpu_id] = 0
                    
                    if task.run_timer > 0:
                        task.state = TaskState.RUNNABLE
                        if self.policy:
                            self.policy.runnable(task, 0)
                            self.policy.enqueue(task, 0)
                    else:
                        task.state = TaskState.SLEEPING
                        task.sleep_timer = task.sleep_burst

        # Dispatching
        for cpu_id in range(self.num_cpus):
            if self.cpus[cpu_id] is None:
                if self.local_dsqs[cpu_id]:
                    next_task = self.local_dsqs[cpu_id].pop(0)
                    self.assign_task_to_cpu(next_task, cpu_id)
                elif self.policy:
                    self.policy.current_cpu = cpu_id
                    self.policy.dispatch(cpu_id, None)
                    # Check if dispatch moved something to local DSQ
                    if self.local_dsqs[cpu_id]:
                        next_task = self.local_dsqs[cpu_id].pop(0)
                        self.assign_task_to_cpu(next_task, cpu_id)


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
