# Specification: sched-ext Discrete Event Simulator

## 1. Project Objective

This a Python-based discrete event simulator for the Linux `sched-ext` (eBPF) scheduling framework. The simulator mocks the `sched-ext` kernel API so that C-based BPF scheduling policies can be translated to Python with a 1:1 structural mapping.

The primary features are:

1. Simulated a synthetic workload of two applications running forever.
2. Replicated a specific BPF scheduling policy (CPU Isolation & Work Conservation).
3. Output a `trace.json` compatible with Perfetto UI for visualization.
4. Implemented a state-dumping assertion to catch "Priority Inversion" corner cases.

## 2. Core Architecture

The system follows a strict decoupling between the Simulation Engine (representing the Linux Kernel) and the Scheduler Policy (representing the BPF program).

### Time Representation

* The simulator runs on a tick-based discrete event loop.
* 1 Tick = 1 us.
* All time slices (e.g., 50ms) must be converted to ticks (e.g., 50,000 ticks) internally.

---

## 3. Component Design

### Part 1: Workload Model

Assume the workload consists of 2 applications which run forever. The only configuration parameter is the number of active threads per application.

* **Task Class**:
* `id`: int (PID)
* `name`: str (e.g., `"critical_0"`, `"hog_1"`)
* `state`: Enum (`RUNNABLE`, `RUNNING`)
* `slice_left`: int (Ticks remaining before eviction)


* **App Generation**:
* The engine initialized two apps: `App X (critical)` and `App Y (hog)`.
* Instantiated 2 tasks for `critical` and 4 tasks for `hog` based on input parameters.



### Part 2: Sched-Ext API (The Interface)

The simulator exposed a base class `SchedExtOps` that user-defined policies will inherit from.

**Callbacks (Triggered BY Engine, IMPLEMENTED by Policy):**

* `init(self)`: Called at startup.
* `select_cpu(self, task, prev_cpu: int, wake_flags: int) -> int`: Returns target CPU.
* `enqueue(self, task, enq_flags: int)`: Logic to place task in a DSQ.
* `dispatch(self, cpu_id: int, prev_task)`: Logic to assign a task to the idle CPU.
* `running(self, task, cpu_id: int)`: Hook for visualization.
* `stopping(self, task, cpu_id: int, runnable: bool)`: Hook for visualization.

**Helpers (Provided BY Engine, CALLED by Policy):**

* `scx_bpf_create_dsq(dsq_id: int, node_id: int)`: Registers a double-ended queue.
* `scx_bpf_dsq_insert(task, dsq_id: int, slice_us: int, enq_flags: int)`: Inserts a task into a DSQ. Supports `SCX_ENQ_HEAD` (append left) and default (append right).
* `scx_bpf_dsq_move_to_local(cpu_id: int, dsq_id: int) -> bool`: Pops a task from the specified DSQ and assigns it to the CPU. Returns True if successful.
* `scx_bpf_kick_cpu(target_cpu: int, flags: int)`: Marks the task currently on `target_cpu` to be evicted at the end of the current tick (Preemption).

### Part 3: The Engine Loop (`KernelSimulator`)

The `KernelSimulator` manages `C` CPUs (default 4). In each tick, it performs the following strictly ordered steps:

1. Advance Time: `self.tick += 1`
2. Execution & Preemption: For each CPU running a task:
* Decrement `task.slice_left -= 1`.
* If `slice_left == 0` OR the CPU was "kicked" in the previous tick:
* Trigger `policy.stopping(task, cpu_id, True)`.
* Change state to `RUNNABLE`.
* Trigger `policy.enqueue(task, 0)`.
* Remove task from CPU.


3. **Dispatching**: For each IDLE CPU:
* Trigger `policy.dispatch(cpu_id, None)`.


4. **Assertion Check**: Run the Priority Inversion Detector.

### Part 4: Priority Inversion Detector (Action Item)

Implemented an assertion in the simulator that tells you the state of the scheduler when a priority inversion happens.

**Definition of Inversion:**
At the end of any tick, if ANY `critical` task is sitting in a queue (`RUNNABLE` state), AND `CPU 0` or `CPU 1` is currently `RUNNING` a `hog` task.

**Action:**
If detected, halt the simulation, print `[ASSERTION FAILED] Priority Inversion Detected!`, and completely dump:

* Current Tick.
* State of all CPUs (What task is running, how much slice left).
* State of all DSQs (List of tasks waiting).

### Part 5: Visualization Exporter (Perfetto)

When `policy.running` and `policy.stopping` are called, the engine will record the start time and calculate the duration.
At the end of the simulation, the engine will export a list of dictionaries to `trace.json`:

```json
{
  "name": "critical_0",
  "cat": "sched",
  "ph": "X",
  "ts": <start_tick_in_us>,
  "dur": <duration_in_us>,
  "pid": 0,
  "tid": "CPU 0"
}

```

---

## 4. The Target Policy to Implement

We created `MyPolicy(SchedExtOps)` that mimics the BPF C code's behavior(`scx_simple_bpf.c`):

* **Constants**: `DSQ_VIP=1`, `DSQ_HOG=2`, `DSQ_NORMAL=0`. Slices: `VIP=50000us`, `NORMAL=10000us`, `HOG=1000us`.
* **Init**: Create the 3 DSQs.
* **Select CPU**: If task is `critical`, return `prev_cpu` if it's 0 or 1, else return `0`. Call `scx_bpf_kick_cpu(target_cpu, 0)`.
* **Enqueue**:
* If `critical`, insert to `DSQ_VIP` with `slice=50000`, flag=`SCX_ENQ_HEAD`.
* If `hog`, insert to `DSQ_HOG` with `slice=1000`, flag=`0`.


* **Dispatch**:
* If `cpu_id` in `[0, 1]`: Try `DSQ_VIP`, then `DSQ_NORMAL`, then `DSQ_HOG`.
* If `cpu_id` in `[2, 3]`: Try `DSQ_NORMAL`, then `DSQ_HOG` (NEVER `DSQ_VIP`).



## 5. Execution Entry Point

```python
if __name__ == "__main__":
    simulator = KernelSimulator(num_cpus=4)
    policy = MyPolicy()
    
    # 2 applications which run forever. Config param: number of threads.
    simulator.add_workload("critical", num_threads=2)
    simulator.add_workload("hog", num_threads=4)
    
    simulator.attach_policy(policy)
    simulator.run(duration_ticks=5_000_000) # Run for 5 seconds
    simulator.export_perfetto("trace.json")

```