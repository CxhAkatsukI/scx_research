# 内核依据与任务登记释放

调研固定在上游 **Linux v6.16**，用于确定可信 API spec 的参考语义。它不是
当前加载机器的版本声明。`scripts/fetch_kernel.py` 从 GitHub API 下载完整
源文件，并校验 Git blob SHA-1（包含 Git blob header）：

| 文件 | blob SHA-1 |
|---|---|
| `kernel/sched/ext.c` | `7dd5cbcb7a069deea2789a1bad78d5f21770a6d9` |
| `kernel/sched/core.c` | `81c6df746df17b88b1d5bc5a61c5c9d98e07084f` |
| `kernel/sched/syscalls.c` | `547c1f05b667ef1a381989bb984015c7c3838344` |

下面链接的行号来自下载文件的实际行号。

## 插入请求与实际生效

enqueue 上的 `scx_bpf_dsq_insert()` 经 `scx_dsq_insert_commit()` 记录 direct-dispatch
决定，调用 `mark_direct_dispatch()`，而非立即修改 user DSQ。callback 返回
后，`do_enqueue_task()` 才处理 `direct_dispatch()`；其后
`dispatch_enqueue()` 链入队列并更新 DSQ count。

依据：[请求路径](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L6290)、
[callback 返回后的分支](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2298)、
[direct dispatch](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2184)、
[实际修改 DSQ](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L1941)。

因此必须分别建模 `Register → RequestInsert → CallbackReturn → KernelCommit`。
N 在 Register 的 CAS 就增加，并持续覆盖整个 pending 窗口。对于 dispatch
callback，插入还可能进入 dispatch buffer；本原型不在 dispatch 中发出新的
user-DSQ 插入，避免把两条 API 路径合成一个 spec。

## 为什么不用 dequeue / quiescent 配对

`dispatch_enqueue()` 的 `SCX_ENQ_CLEAR_OPSS` 分支清除 ops state。
`ops_dequeue()` 只在特定 `SCX_OPSS_QUEUED` 分支调用 `ops.dequeue`，所以
“每次 DSQ 任务离开都会触发 dequeue”不是有效前提。

更关键的是 `dequeue_task_scx()` 先调用 `quiescent`，最后才调用
`dispatch_dequeue()`。在 quiescent 中释放可能使仍在 user DSQ 内的任务失去
accounting。迁移中的 dequeue 又可能跳过 quiescent。

依据：[清除 ops state](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2035)、
[dequeue 的条件分支](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2412)、
[quiescent 与 unlink 的顺序](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L2459)。

## 保守释放事件

| 事件 | 为什么可以释放旧登记 | 实现处理 |
|---|---|---|
| `running(p)` | `set_next_task_scx()` 先处理 `ops_dequeue` 和 `dispatch_dequeue`，再调用 running；该代任务已离开原 user DSQ | 释放该 task-storage 的 owned ticket |
| 再次 `enqueue(p)` | 同一 task 的旧入队已经结束并移除；rq 锁排除旧的 direct-dispatch commit 跨过此次调用 | 先释放旧 ticket，再为新目标登记新 generation，最后请求插入 |
| `disable(p)` | class 切换先通过旧 class 的 dequeue；`switched_from_scx()` 再调用 disable | 释放；再次 disable/exit 不重复减 |
| `exit_task(p)` | fork 取消可能从未登记；正常销毁或 scheduler disable 时，旧排队责任已经结束 | owned 时释放；成功后删除 task storage |
| 成功 move 到 local DSQ | 任务已离开 user DSQ，但 bool 返回值没有告诉调用者具体 task；迁移路径可能不通知 | **不释放**，由后续 running/安全移除处理 |
| 取消、迁移、quiescent | callback 可能先于实际 unlink，或者通知被跳过 | **保留**，等待之后的安全事件 |

依据：[running 前的移除](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L3246)、
[class 切换中的 dequeue 和回调](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/core.c#L7242)、
[sched_setscheduler 路径](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/syscalls.c#L703)、
[switched_from](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L4054)、
[disable / exit_task](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L3880)、
[scheduler 退出时同步清理](https://github.com/torvalds/linux/blob/v6.16/kernel/sched/ext.c#L4937)。

这些依据支持选择安全释放点，**没有机器证明目标内核的全部调用路径**。
任务回调的序列化、旧 generation 不会再次落队、task-storage 与 task object 的
生命周期绑定，均在当前 trusted adapter 的前提中明确列出。目标版本改变时
必须重新核对。

## N 的含义与代价

N 包括已登记未提交、实际 queued、已移到 local DSQ，以及已经移除但暂时没有
安全通知可释放的任务。后两类记为 Detached。因此不承诺 `N == Q`；承诺的是
`Q <= N`，并在能够确认的安全 callback 中恰好释放一次。

取消后一直睡眠的任务可能长期保留一个 ticket，直到再次 enqueue、disable
或 exit。这会保守地保留 D，影响效率，但不会隐藏真实 queued 工作。计数满
时，新 enqueue 在发出 user-DSQ 请求前改走内核 GLOBAL DSQ。该策略不依赖
计数最终必然回到零来维持安全，也不把未实现的释放 callback 当作模型假设。

任务身份使用 task storage，不使用 PID；generation 单调递增并拒绝溢出。
host 实现另有不复用的 partition/task ID，可检出错误任务、错误队列和过期
generation 的释放。BPF 只有一个 partition，当前 task 指针及私有 storage
提供身份，且没有可提交任意旧 ticket 的异步释放入口。
