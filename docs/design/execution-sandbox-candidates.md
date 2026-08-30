# 执行沙盒候选方案（待决策）

> **状态：候选录入，尚未选型落地。**  
> 记录对「Agent 执行隔离」的讨论结论，便于后续拍板。实现前勿默认接入任一方案。  
> 历次讨论展开（本机候选、landstrip、命令边界、Cube 工作量）见
> [execution-sandbox-discussion.md](execution-sandbox-discussion.md)。

## 背景与目标

Pointer 当前：

- `session-sandboxes/` 只是 **app-data 目录约定**，不是 OS / 网络隔离。
- `terminal`、文件工具、Computer 默认作用在主机（或 server 进程所在机）。
- 异步子 Agent 设计里 **P3** 写的是 worktree 写冲突隔离，并明确 **不做云 VM**（见 [async-subagent-and-terminal.md](async-subagent-and-terminal.md)）——那是另一条「同仓并行写」问题，与本文「防主机受损 / 多租户隔离」可并行存在。

产品上希望有一条 **可选** 的执行隔离能力（默认仍可本地）。讨论中收敛出两条不同产品线，不要混成一个方案：

| 线 | 目标 | 典型形态 |
|----|------|----------|
| **A. 本机轻量沙盒** | 桌面 App（macOS / Windows / Linux）防误写盘、乱跑 shell、偷读密钥 | Seatbelt / Landlock / AppContainer，或 landstrip 一类封装 |
| **B. Server 强隔离** | Linux `pointer-server` / Web 多租户，防不可信 Agent 伤宿主机 | MicroVM / 远程沙盒集群（如 CubeSandbox） |

Computer（真机鼠键/截屏）在轻量本机沙盒下 **基本防不住**；沙盒模式应禁用 Computer，或另开虚拟桌面。

## 候选 B：CubeSandbox（Linux server）

上游：[TencentCloud/CubeSandbox](https://github.com/TencentCloud/CubeSandbox)  
定位：基于 RustVMM + KVM 的 AI Agent 沙盒服务，硬件级隔离，对外 **兼容 E2B SDK**（改 `E2B_API_URL` 可切换）。宣称毫秒级启动、高密度、出站安全代理与凭证注入。

### 适合什么

- **仅考虑 Linux server** 时：与「可选强隔离执行环境」目标 **对口且偏强**。
- 多租户 / 不可信代码执行：独立客户机内核，隔离高于同内核的 bwrap / Landlock。
- 能力面：命令 / PTY、文件系统、Volume、出站策略（CubeEgress）、快照/暂停恢复等，适合会话租一台小 VM。

### 不适合什么

- **桌面 App 本机轻量开关**：不能塞进 Tauri 进程；Mac / Win 需另外部署 Linux 节点。
- 「零运维、无集群」：需要控制面 / 计算节点（或同机 KVM）、模板与 DNS/代理运维。
- 替代异步子 Agent 的 **worktree** 方案：Cube 是远程执行平面，不是 git worktree。

### 与 landstrip / OS 原语（server 上）对比

| | landstrip / bwrap 等 | CubeSandbox |
|--|----------------------|-------------|
| 隔离 | 中（同内核） | 高（MicroVM） |
| 部署 | 轻，主要包 `terminal` | 中高，集群或单机 KVM + 模板 |
| 延迟 | 近乎本机 | API + 进 VM |
| 多租户互害 | 较弱 | 较强 |
| 接 Pointer | 改 spawn 一层 | 整层 FS / shell 远程化 |

粗建议（仍待决策）：

- Server **多租户 / 防恶意** → 优先评估 Cube（或同类 E2B 兼容集群）。
- Server **单租户 / 防误操作** → 更轻的 bwrap / landstrip 可能足够。

### 若选型 Cube，接入 Pointer 时仍要做的事

1. **工具后端分流**：`file_*` / `terminal`（含后台 terminal job）→ Cube 会话；进程内直写主机 FS 关闭或只读。
2. **工作区同步**：Volume 挂载 vs 按需 sync（大仓、权限、`.git`）。
3. **生命周期**：会话绑定 sandbox；停止 / 超时回收；与 JobSupervisor cancel 对齐。
4. **Computer**：沙盒模式默认关，或仅验证无头浏览器类模板。
5. **密钥**：走出站代理注入，避免明文进 VM 环境变量。

产品形态示意：

```text
默认：pointer-core 工具 → 本机 FS / shell
可选：pointer-core 工具 → Cube（E2B API）→ MicroVM 内 FS / shell
```

## 候选 A：本机轻量沙盒（摘要）

讨论中曾列：

| 方案 | 隔离 | 跨端 | 备注 |
|------|------|------|------|
| 应用层软策略 | 低 | 三端易 | 最快，非内核隔离 |
| OS 原语（Seatbelt / Landlock / AppContainer） | 中高 | 三端各一套 | 业界 coding agent 常见路 |
| landstrip 等封装 | 中高 | 三端 | 适合包 `terminal`；LGPL 等分发约束需法务 |
| 本机 Docker / 轻量 VM | 中～高 | 有依赖 | 偏重，非「轻量开关」首选 |

**不建议**用 Cube 满足线 A。线 A 与线 B 可并存：App 走 A，server 走 B。

展开（候选对比、landstrip 深读、命令名黑名单边界、spike 清单）见
[execution-sandbox-discussion.md](execution-sandbox-discussion.md) §4–§6。

## 决策记录（空）

| 日期 | 决定 | 备注 |
|------|------|------|
| （待填） | 未选型 | 本文仅录入候选 |

拍板时建议写清：

1. 只做 A、只做 B，还是 A+B。
2. Server 默认 `local` 还是可选 `cube`。
3. 与 [async-subagent-and-terminal.md](async-subagent-and-terminal.md) P3 worktree 的边界（写冲突 vs 执行隔离）。

## 参考

- 讨论纪要：[execution-sandbox-discussion.md](execution-sandbox-discussion.md)  
- CubeSandbox：https://github.com/TencentCloud/CubeSandbox  
- E2B 兼容接入思路：上游文档 Quick Start / OpenAI Agents SDK 集成说明  
- 本仓库相关：`docs/developer/workspace-root.md`（session-sandboxes 目录语义）、[async-subagent-and-terminal.md](async-subagent-and-terminal.md)（P3 不做云 VM 的上下文）
