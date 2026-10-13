# Problem 1：保护延迟落队窗口的 published accounting

这一目录交付 Problem 1 的协议实现、内核生命周期调研、共享操作的语义
contract，以及连接到生命周期模型的证明。代码独立于旧的 reproduction、
mocked-adapter 和 `lachesis_step1`，便于逐文件审阅。

核心变化是：**登记先于插入请求，N 包含尚未落队的工作；C、D、N 通过同一
原子字更新。** 不再把 `dsq_insert()` 返回视为实际入队，也不把消费成功、
消费失败或 `quiescent` 视为可减计数的凭证。

当前范围是每个 partition 两个 LLC，每个 N 最多 65,535。BPF 原型实现一个
partition、两个 user DSQ；CPU 到 LLC 的映射必须在加载前按目标机器设置。
这里的 C 是两个 LLC 的完整覆盖配置，不代表任意 CPU mask 的完整实现。

## 从哪里开始审阅

| 文件 | 内容 |
|---|---|
| [docs/kernel-lifecycle.md](docs/kernel-lifecycle.md) | v6.16 调用链、释放时机、可信前提 |
| [docs/contracts.md](docs/contracts.md) | callback 职责、原子步骤、连接关系与限制 |
| [docs/validation.md](docs/validation.md) | 本次工具版本、运行证据、内核/BTF 身份和复现命令 |
| [src/word.rs](src/word.rs) | 唯一一份计数与 bit 操作实现，供 Rust host 和 BPF 共用 |
| [src/lib.rs](src/lib.rs) | host callback accounting、任务/队列/代际身份校验、CAS 重试 |
| [bpf/callbacks.bpf.c](bpf/callbacks.bpf.c) | 真实 struct_ops callback 原型与 task storage |
| [verify/](verify/) | 精确 bit 操作语义、无界生命周期、二者的连接证明 |
| [tests/](tests/) | 逐任务交错检查、实际 callback 的 mock-kernel 测试、并发和边界测试 |

## 本地复现

```sh
cd problem1_accounting
make check
make build
# 显示交错覆盖数与四个错误协议的最短反例：
cargo test --test interleavings -- --nocapture
# 可选：下载并校验调研所用的内核源文件，不参与模型证明：
python3 scripts/fetch_kernel.py
```

`make check` 需要 Rust/Cargo、Clang 23（含 sanitizer runtime）和 Verus。
Verus 默认复用 `../lachesis_step1/dep/verus/source/target-verus/release/verus`；
可通过 `VERUS=/absolute/path/to/verus` 覆盖。当前检验使用 stage1 已固定的
Verus `5f6eb59`、Rust 1.98.0 和 Z3 4.16.0；host Cargo 使用 Rust 1.94.1。

离线构建 BPF ELF：

```sh
make bpf VMLINUX=/absolute/path/to/target/vmlinux
```

也接受 raw BTF。构建复用 stage1 的 BPF target 和已编译的 `libcore`，并需要
匹配的 Rust 1.98.0、LLVM/Clang 23、bpftool、libbpf 开发头文件。
`BPF_RUSTC`、`BPF_RUST_DEPS`、`CLANG`、`BPFTOOL` 可覆盖工具路径。
输出都在被忽略的 `build/`、`target/` 中，上述检查和离线构建命令不会加载 scheduler。

VM 检查使用 QEMU/KVM、virtme-ng、4 个 vCPU；默认复用 stage1 的 `vng`，
并使用 `/boot/vmlinuz-linux`。可用 `VNG`、`KERNEL` 指定其他路径：

```sh
make vm-verify  # 只执行 verifier，不 attach scheduler
# 若 BTF 不同，上一条会导出 guest BTF 并退出；据此重新构建：
make bpf VMLINUX="$PWD/build/vm/vmlinux.btf"
make vm-verify
make vm-smoke  # 在 QEMU 内短时 attach，只有 8 个 worker 切入 SCHED_EXT
```

VM 命令核对 guest BTF 与构建记录。运行测试把 vCPU 0、1 配置为两个**合成
LLC 组**，vCPU 2 承载控制进程；这不代表物理 LLC 拓扑。运行测试有超时，
退出时卸载并检查 disabled，脚本与 loader 均拒绝在 QEMU 之外执行。

## 本次验证结果

| 层次 | 结果 | 能支持的结论 |
|---|---|---|
| Verus | **25 verified, 0 errors**，启用 `--no-cheating` | 实际 Rust word 函数符合语义 contract；无界生命周期保持不变量；这些 word 效果与模型步骤保持连接关系 |
| 证明负例 | 4 个实际函数体错误变体均产生证明错误 | contract 能发现丢 drain、覆盖其他 bit、减错位置和忽略 pending 等错误 |
| 逐任务模型检查 | **2,410 状态、20,920 转换** | 两任务、两 LLC 的完整有限状态空间中，连接关系及安全性质成立；允许任意次数重入队和已确认不会迟到 commit 的 pending 取消 |
| 协议反例 | 4 个错误协议均到达 stranded | 反例包含独立的 request、callback return、kernel commit，能检出被合并模型藏住的窗口 |
| Rust host | 6 项测试通过 | 身份/代际/队列检查、饱和、非法配置、并发 CAS、pending 窗口 |
| 实际 C callbacks + Rust word | 3 组生命周期场景通过，ASan/UBSan 开启 | 相同 callback 函数体在显式延迟的 mock kernel 中符合预期；包含成功/失败消费、PID 重用、disable/exit 和再次 enqueue |
| BPF ELF | 编译成功，检查到 `cmpxchg`，无遗留 Rust/core 未定义符号 | 实际 Rust 实现已链接进 BPF callback 对象 |
| QEMU verifier | Linux **7.2.9-arch1-1** 接受全部程序 | 对象在这一测试内核上通过 verifier；这一检查没有 attach scheduler |
| QEMU 运行 | 8 个 SCHED_EXT worker、1,000 次成功配置切换、1,000 次合法 word 采样，结束后 N=0、卸载后 disabled | 真实 callbacks/task storage 在有迁移、睡眠/唤醒、再次 enqueue 和 disable/exit 的短时负载中工作；不等于穷尽并发窗口 |
| 真实目标内核 / 完整连接证明 | **尚未完成** | 不声明已消除实际部署内核中的全部竞争，也不声明验证了整个 C callback 到 kernel 的连接 |

本次最终 ELF 使用 VM 导出的 7.2.9-arch1-1 BTF；**生命周期调研基线仍是
Linux v6.16**，两者没有被当作同一目标。运行日志和对象身份见 validation。
当前宿主机是 WSL 6.6.87.2，未在宿主机加载 BPF scheduler。

## 剩余集成工作

实际加载前需要固定目标 kernel commit/BTF，复核任务锁与这些安全释放事件，
配置真实 LLC 拓扑，在目标版本重跑 VM 检查，并采集实际 pending 与取消/迁移
窗口的逐任务证据。当前 VM 压力采样没有观测每个任务的 KernelCommit，也
没有强制复现特定 pending 窗口。C callback 的 CAS/task-storage 绑定目前属于
可信边界；形式化连接证明覆盖其 word 效果和抽象生命周期，没有验证整个 C
callback 的控制流或 Linux 本身。

这一版证明的是安全性质。Detached 的保守登记可能一直留到后续安全 callback；
drain 的及时退休、CPU 唤醒、任务最终得到运行及多 partition 的公平性尚无证明。
