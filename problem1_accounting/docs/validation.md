# 验证记录（2026-10-11）

## 源码、工具与证据位置

本目录的生成物均留在被忽略的 `build/`、`target/` 中，没有把依赖源码、
生成 skeleton、ELF 或大日志加入交付源码。旧实验目录未修改。

| 用途 | 本次版本 |
|---|---|
| 生命周期调研 | Linux v6.16；三个完整源文件的 Git blob 身份见 kernel-lifecycle |
| host | WSL `6.6.87.2-microsoft-standard-WSL2`；Rust 1.94.1 |
| Verus | `5f6eb59`；Rust 1.98.0；Z3 4.16.0；`--no-cheating` |
| BPF | Rust 1.98.0 + LLVM/Clang 23.1.1；与 C bitcode 链接后生成 BPF ELF |
| VM | QEMU/KVM + virtme-ng；4 vCPU、2 GiB；`7.2.9-arch1-1` |

VM 内核由本机 `/boot/vmlinuz-linux` 启动。它是具体的测试构建，不冒充 v6.16
或项目将来的部署内核。对应 SHA-256：

```text
kernel image:
d02afb36d5f99390fdd8d04dedf8894354f4bbdc5e7d44afa5b897639bb3707c
guest raw BTF:
e707a907dffccbfd0e3e6caa1ab7c9bec6d7f1466235f4ce4e217837cb95fc42
```

`build/bpf/build-info.txt` 记录本次输入、raw BTF 和对象 hash。VM runner 要求
对象仍匹配该记录，guest BTF 也必须相同；不匹配则停止。ELF 输入先提取
`.BTF` 再核对。`vm-smoke` 每次重新生成 loader，避免沿用旧 skeleton。

## 已完成检查

```sh
make check
make build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --test interleavings -- --nocapture
make bpf VMLINUX="$PWD/build/vm/vmlinux.btf"
make vm-verify
make vm-smoke
```

`make check` 包含 8 项 Rust 测试、3 组执行实际 C callback 的 mock-kernel
生命周期测试，以及实际 Rust 函数体的 Verus 验证和 4 个证明负例。
mock-kernel 的 C 路径开启 ASan/UBSan；链接的 Rust 对象没有 sanitizer 插桩。
逐任务模型共有 2,410 个可达状态、20,920 个转换，包含 request/return/commit
分离与显式取消证言。它不探索任意数量任务、CPU 或任意 kernel 行为。

Verus 输出 `25 verified, 0 errors`；4 个错误函数体各产生证明错误。
缺工具、语法错误、类型错误不会算作有效负例。日志位于 `build/verify/`。

VM verifier 日志位于 `build/vm/verifier.log`，结束时核对 scheduler 没有被
启用。运行测试日志位于 `build/vm/smoke.log` 和 `smoke-run.log`。它只把 worker
切入 SCHED_EXT，控制进程保留 CFS，并在退出后核对 sched_ext 为 disabled。

运行测试有 8 个 worker，执行计算、yield、睡眠/唤醒，其中一个反复迁移；
控制进程经真实 `p1_config` BPF syscall 程序提交 1,000 次 coverage 更新。
每次采样只原子读取 packed word，并检查它的合法性及 `N>0 && !C ==> D`。
要求观察到非零 N 和 D，所有 worker 取得进展并正常离开 SCHED_EXT，最后
N=0，卸载前仍为 enabled、卸载后为 disabled。采样不会写入 Q、C、D 或 N。

本次通过的运行输出如下；进展数和采样分布会随运行变化：

```text
OK: workers=8 progress=258423 config_updates=1000 samples=1000 busy_samples=874 drain_samples=818 final_N=0
OK: scheduler detached; sched_ext state=disabled
```

## 结果边界

Verifier 接受不代表协议正确；短时采样不会穷尽 interleaving。VM 没有直接
记录每个任务的 pending/KernelCommit，也没有用真实 DSQ Q 核验完整的逐任务
连接关系。已证明的是 word 函数、无界 aggregate 生命周期及它们之间的连接；
kernel、完整 C callback 控制流、task storage 绑定和安全释放证言仍在可信边界。

实验把两个 vCPU 映射为两个合成 LLC 组，没有声称它们是两个物理 LLC。
C 是两个组的完整 coverage 配置，没有实现任意 CPU mask 或多 partition
全局事务。保守 Detached accounting 可延迟 drain 退休；活性、公平性和
实际部署版本的全路径正确性留待后续集成。
