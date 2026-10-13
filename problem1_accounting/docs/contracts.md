# Callback 与共享操作的语义 contract

## 状态、范围与连接关系

一个 partition 的原子字布局如下，所有未使用 bit 必须为零：

```text
63                 36 35            20 19             4 3  2 1  0
+--------------------+----------------+----------------+----+----+
|       zero         |    N[1]:16     |    N[0]:16     | D  | C  |
+--------------------+----------------+----------------+----+----+
```

每次原子提交均保持 `N[l] > 0 && !C[l] ==> D[l]`。
生命周期中区分 Pending、Queued、Detached，保持
`N = Pending + Queued + Detached`。Pending 也包含 callback 已完成登记、尚未
请求插入的阶段；Detached 包含 local DSQ 和移除后仍保留的登记。

连接关系把模型的 registered/covered/draining 与**同一个 partition、同一个
LLC** 的 word 字段绑定。`verify/bridge.rs` 证明实际 word 效果分别连接到
Register、Publish、Release、Retire，Land/CancelPending/Move/Remove 不改变
word 字段。CancelPending 只在 kernel 确认这代请求不可能迟到落队后，将
Pending 转为 Detached；不能从取消意图或 quiescent 自动推出这个确认。
由 `Q <= N` 和上述不变量推出 `Q > 0 && !C ==> D`，无需把一个全局并发状态
当作单个 callback 自己的出口义务。

当前连接证明的生命周期状态是无界 aggregate 模型；逐任务身份由有限模型
检查和运行测试覆盖。尚未形式化证明 C callback 的所有局部阶段、task storage
和 kernel 的全局连接关系。

## 共享操作

以下效果发生在**成功 CAS 的线性化点**，描述本次操作的贡献，不表示其他 CPU
在整个调用前后都不能修改字段。CAS 失败不执行这一模型步骤，必须以实际返回
的 word 重新计算；BPF 重试 32 次仍失败时返回失败，不伪造成功。

| 操作 | 前提 | 原子效果 |
|---|---|---|
| Register(l) | 合法且安全的 word；`l < 2`；`N[l] < 65535`；当前 task 无其他 owned ticket | `N[l] += 1`；`D[l] := D[l] || !C[l]`；其他 count、其他 D、C 保持 |
| Publish(c) | 合法且安全的 word；`c < 4` | 一次发布完整 C；每个 l 的 `D[l] := D[l] || (N[l]>0 && !c[l])`；N 保持 |
| Release(l) | 正确 task/partition/LLC/generation 的 ticket；这代任务不在 user DSQ 且不可能迟到落队；`N[l] > 0` | `N[l] -= 1`；C、D、其他 count 保持；该 ticket 不再可释放 |
| Retire(l) | 合法且安全的 word；`l < 2` | 只在同一快照 `N[l] == 0` 时清该 D；其他字段保持 |

`src/word.rs` 的 `//%` 是 Verus annotation，host 编译时是注释。验证脚本只
激活这些 annotation、命名返回值并加入 spec/proof，不替换算法函数体。
BPF 通过 `bpf/core.rs` 编译**相同源码**并与 C callbacks 的 LLVM bitcode
链接；没有另一份手工 C bit 算法。证明负例直接修改这些实际函数体。

## Callback 职责与执行阶段

**Enqueue：**入口要求 task/context 合法、同一 task 的生命周期操作串行，旧
generation 已经移除。若旧 ticket 仍 owned，先在安全点释放。随后选择合法
LLC，成功 Register，写入当前 task 的 `(LLC,generation,owned)`，最后发出一次
user-DSQ 插入请求。出口可保证请求与登记绑定，**不保证已经 Queued**。
counter 满或 CAS 重试耗尽时，在 user-DSQ 请求之前走 GLOBAL fallback。
没有在 select_cpu 中绕过登记插入 user DSQ 的路径。

Register 的 CAS 与 task-storage 写入之间，局部阶段是 Reserved；此时 N 已
增加，但 kernel 尚不能落队。同一 task 的序列化保证另一条释放路径不能在
此时使用未完成记录。Release 的 CAS 与清 owned 之间也有局部阶段；callback
返回前清 owned，其他 task 不会使用它。**当前这一局部阶段映射属于 adapter
的可信义务，还没有完整 C 控制流证明。**

**Config：**BPF `p1_config` 接收一个 u64 的完整 coverage mask，仅接受 0..3。
一次 Publish CAS 同时发布配置和全部必要 D；失败返回 `-EAGAIN`，非法值返回
`-EINVAL`。配置的可信写入端只允许调用该 syscall BPF program，不允许直接
改 `.data` 中的 packed word。当前没有另一个 CPU-mask map，因而不声称解决
了 coverage 与任意外部 mask 的原子发布问题。

**Dispatch/drain：**只在本 LLC 有覆盖，或目标 LLC 的 D 开启时尝试 move。
move 成功由 kernel 转移一个具体 task，调用者仅收到 bool，不据此释放任意
ticket；失败不推出 Q=0。两种结果之后都只能通过 Retire 检查 N；pending
仍存在时 D 不能被稳定清除。Retire 重试失败则保留原状态，等待之后重试。

**Running / disable / exit / 再次 enqueue：**只释放当前 task 私有 storage
中的 owned ticket。成功减计数后清 owned；重复 callback 不再减。未确认移除
时不释放。BPF 中这些安全点来自内核版本核查；host API 的 `removed` 参数是
显式的调用者证言，不是库自行观测到的 kernel 事实。

## 内存序与可信边界

host 的 CAS 使用 Relaxed：对这一**单一原子位置**的读改写顺序足以保持 word
不变量，无需假定多个独立位置顺序一致。它不负责发布 task memory，也不与
DSQ 锁建立同步。C adapter 使用 `__sync_val_compare_and_swap`，原子读取也通过
CAS；离线反汇编检查确实生成 `cmpxchg`。不将 host Relaxed 的推理自动当作
Rust→LLVM→BPF→目标架构编译器的完整正确性证明。

此外必须信任：成功登记先于插入 kfunc；所有该 partition user-DSQ 插入经过
登记；DSQ ID 不被销毁/复用；task hooks 及 storage 被内核正确序列化；安全
释放后该 generation 永不再次落队；消费只转移一次；未加载前初始化 word
为 N=0/D=0 的合法值。目标机器的合法 task、CPU、affinity 与 kernel 版本
来自环境，不是 Verus 为真实机器自动建立的事实。

## 不承诺的性质

当前结果没有证明 kernel 本身、整个 C callback、无限任务身份空间的逐任务
连接、无饥饿、CAS 的有限等待、drain 的及时退休，或 CPU kick 的活性。
两 LLC 的 packed layout 也不直接推广到任意数量 LLC，多个 partition 的
配置没有跨 partition 原子提交。

待目标版本和完整 adapter 连接建立后，才可以把这里的条件式安全结论用于
真实运行实例。VM verifier 和短时运行测试也不能替代那一步。
