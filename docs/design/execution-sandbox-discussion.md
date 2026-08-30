# 执行沙盒讨论纪要（候选探讨）

> **状态：讨论纪要，非已拍板实现方案。**  
> 汇总历次「Agent 执行隔离 / 防主机受损」讨论的主要结论，以及本机轻量候选与
> landstrip 深读。正式候选表与拍板栏见
> [execution-sandbox-candidates.md](execution-sandbox-candidates.md)。

## 1. 问题从哪来

产品诉求（讨论口径）：

- **默认仍本地工作**；
- 希望有 **可选项**：把易伤主机的本地化操作关进沙盒，降低误写盘、乱跑 shell、
  偷读密钥的风险。

与现状的差距：

| 现状概念 | 实际含义 |
|----------|----------|
| `session-sandboxes/` | app-data **目录约定**（工作区 / 附件落盘），**无** OS / 网络隔离 |
| `terminal` / `file_*` | 在跑 `pointer-core` 的机器上直接 `std::process` / `std::fs` |
| Computer | 操作 **真机** 桌面（鼠键 / 截屏 / 剪贴板） |
| 云主机 | **整机**迁到远程 pointer-server，不是按会话的轻量沙盒 |

异步子 Agent 文档里的 P3 **worktree**（见
[async-subagent-and-terminal.md](async-subagent-and-terminal.md)）解决的是
**同仓并行写冲突**，并写明不做云 VM——与本文「执行隔离 / 防主机受损」是
**另一条线**，可并存，不要混成一个方案。

## 2. 主要结论（跨次讨论）

1. **必须拆成两条产品线**，不要用一套技术硬扛两端：
   - **线 A — 本机轻量沙盒**：桌面 App（macOS / Windows / Linux）可选开关。
   - **线 B — Server 强隔离**：Linux `pointer-server` / 多租户，防不可信代码伤宿主机。
2. **Cube Sandbox（E2B 兼容 MicroVM）对口线 B，不对口线 A。**  
   不能塞进 Tauri 本机进程；Mac/Win 用户选 Cube = 连远程 Linux 节点。
3. **「避免主机受损」≠ Computer 自动安全。**  
   轻量本机沙盒 **管不住** 真机 HID；沙盒模式应 **禁用 Computer**，或另开虚拟桌面
  （工作量大、不确定度高）。
4. **OS 沙盒拦的是能力（路径 / 网络），不是命令名字符串。**  
   `rm -rf ~` 靠写策略挡住；`sudo` / `curl|sh` 的「名字」要另做应用层过滤，且可被绕过，
   不能当唯一安全边界。
5. **接隔离的工程主体是 Pointer 工具 spawn / FS 边界**，不是「加个依赖就完事」：
   - 线 A：主要包 `terminal`（及同类子进程），`file_*` 用 write-scope 对齐；
   - 线 B：整层 FS + shell 远程化 + 工作区同步 + 会话生命周期。
6. **失败不得静默回落裸主机执行**（与项目可观测性规范一致）；沙盒拒绝应进工具结果 /
   trap 日志。

## 3. 线 B：Cube / 远程 MicroVM（工作量口径）

上游：[TencentCloud/CubeSandbox](https://github.com/TencentCloud/CubeSandbox)  
对外兼容 E2B SDK（改 `E2B_API_URL` 等即可指向自建集群）。

粗估（熟悉本仓库、1 全栈 + 运维配合）：

| 范围 | 人周量级 | 说明 |
|------|----------|------|
| MVP：可选沙盒，file + terminal + lint；Computer 禁用 | 约 10–16 | 真正降低乱写盘 / 乱跑 shell |
| + 工作区双向同步与面板 | +3–5 | 体验坑点 |
| + Computer 进虚拟桌面 | +6–12 | 依赖 Desktop 类模板，不确定度高 |
| Cube 集群生产运维 | 并行 2–6 起 | 视自建 / 托管 |

若目标只是「别在我笔记本上跑」，现有 **云主机** 已是粗粒度方案；Cube 的价值是
更轻的 **按会话租 VM**、快启停、多租户，不是唯一选项。

接入时仍要做：工具后端分流、工作区 Volume/sync、sandbox 生命周期与 cancel 对齐、
密钥走出站代理注入。细节表见
[execution-sandbox-candidates.md](execution-sandbox-candidates.md)「候选 B」。

## 4. 线 A：本机轻量沙盒候选

目标：可选开关、默认 local、无远程集群、跨三端尽量可用。

| 方案 | 隔离 | 轻量 | 跨端 | 备注 |
|------|------|------|------|------|
| **A1 应用层软策略** | 低 | 极轻 | 易 | 收紧 write-scope、禁 elevated；非内核隔离；1–2 周可出开关 |
| **A2 OS 原语** | 中高 | 很轻 | 三套实现 | macOS Seatbelt、Linux bwrap/Landlock、Windows AppContainer；业界 coding agent 常见 |
| **A3 landstrip 等封装** | 中高 | 轻 | 较好 | 外包 `terminal`；策略兼容 Anthropic sandbox-runtime 子集；见 §5 |
| **A4 本机 Docker / OrbStack 等** | 中 | 中 | 有依赖 | 偏重，非「轻量开关」首选 |
| **A5 本机轻量 VM（VZ / WSL2）** | 高 | 偏重 | 三套 | 强隔离，不像可选轻开关 |

讨论中的落地倾向（**仍待拍板**）：

1. 优先评估 **A3（landstrip 类）+ A1 作兜底**；
2. A3 不达标再退 **A2**（Windows 可降级 soft 或 WSL）；
3. **不要**用 Cube 满足线 A；线 A 与线 B 可并存（App 走 A，server 走 B）。

设置心智示例（未实现）：`sandbox = off | soft | os`  
（`off` 现状；`soft` 仅策略；`os` 包 terminal 子进程。）

## 5. landstrip 深读摘要

上游：[landstrip/landstrip](https://github.com/landstrip/landstrip)

### 是什么 / 不是什么

- **是**：给 coding agent **外包命令进程** 的跨平台 OS 沙盒 CLI（策略 JSON/YAML）。
- **不是**：容器 / MicroVM；也 **不替代** 进程内 `file_*`（除非改成经 shell 写文件）。

形态：`landstrip -p policy.json -- <command...>`  
策略兼容 Anthropic Sandbox Runtime **子集**（与 Claude Code `/sandbox` 同路）。

### 三端机制

| | macOS | Linux | Windows |
|--|-------|-------|---------|
| 后端 | Seatbelt（`sandbox-exec`，官方标废弃仍常用） | Landlock + 必要时 seccomp broker | 默认 **AppContainer（LPAC）**；可选 restricted-user（需一次 UAC setup） |
| FS | 路径编进 profile | 静态 ruleset + 动态补洞 | 每跑一次临时 ACL，结束撤销 |
| 网络默认 | 禁直连；可 proxy / loopback | 同左 | 能力粗（整开/整关）；细粒度 host/port 难 |

### 策略心智

- **写**：默认拒绝 → `allowWrite`（工作区等）。
- **读**：Unix 默认放开，`denyRead` 后变白名单；Windows 需显式 read allowlist。
- **网**：默认禁；`allowNetwork: true` 或域名/proxy 字段。
- `denyWrite` glob：Linux 可动态拦；macOS 多为启动快照；Windows AppContainer 对 glob 支持弱。

### 可观测性

拒绝事件一行一条 JSON（stderr / `--trap-fd`），含 `FILESYSTEM_DENIED` /
`NETWORK_DENIED` / `LAUNCH_FAILED` / `SANDBOX_SETUP_FAILED` 等，便于进工具结果。

### 许可证与分发

- npm wrapper：Apache-2.0  
- **Rust 源码与原生二进制：LGPL-2.1-or-later**  
建议：**捆绑独立 `landstrip` 可执行文件再 `exec` 外包**，避免静态链进闭源主二进制；上线前法务过一眼。

### 接 Pointer 时建议边界

```text
terminal / read_lints / 其它 spawn  → landstrip -p … -- shell…
file_*                              → 仍 pointer-core FS，write-scope 与策略对齐
Computer                            → 沙盒模式禁用
elevated terminal                   → 沙盒模式直接拒绝
```

失败：显式报错，**禁止**静默退回裸 `Command`。

### Spike 清单（约 1 周）

三端各验：工作区外写/读拒绝与 trap；`npm`/`cargo` 网络策略；PTY / cancel /
`WORKING_DIR`；Windows PowerShell（及必要时 Git Bash / restricted-user）；法务分发形态。

## 6. 「能阻止 terminal 执行特殊命令吗」

| 问题 | 结论 |
|------|------|
| landstrip 能否按命令名黑名单拦 `sudo` / `rm -rf /`？ | **一等能力没有**；拦的是写盘 / 联网等效果 |
| `rm -rf ~`、改 `/etc`、读 `~/.ssh` | 配好 FS 策略后，**内核级可拦** |
| 工作区内 `rm -rf .` | **故意允许**（否则无法改代码）；靠 git / 确认 / 备份 |
| 按名字禁命令 | 在 Pointer `terminal` **入口另做软过滤**；防君子，可被编码/间接调用绕过 |
| 与 elevated 的关系 | 沙盒模式应关掉提权路径，避免和 AppContainer 叙事冲突 |

行业同类（Anthropic sandbox-runtime）：路径 + 网络隔离；命令级 allow/ask/deny 属上层 permission，不是 Seatbelt 按 argv 判。

## 7. 与候选表文档的分工

| 文档 | 职责 |
|------|------|
| **本文** | 讨论纪要：结论、候选探讨、landstrip / 命令边界、工作量口径 |
| [execution-sandbox-candidates.md](execution-sandbox-candidates.md) | 精简候选表 + **决策记录**（拍板时填） |

实现前以候选表状态为准；未选型前勿默认接入任一方案。

## 8. 拍板时建议写清的问题

1. 只做线 A、只做线 B，还是 A+B。  
2. 线 A 首选 A3 landstrip、自研 A2，还是先上 A1 soft。  
3. Server 默认 `local` 还是可选强隔离（Cube 或同类）。  
4. 沙盒模式下 Computer / elevated 的硬门禁文案与行为。  
5. 与 async-subagent P3 worktree 的边界（写冲突 vs 执行隔离）。

## 参考

- 候选表：[execution-sandbox-candidates.md](execution-sandbox-candidates.md)  
- CubeSandbox：https://github.com/TencentCloud/CubeSandbox  
- landstrip：https://github.com/landstrip/landstrip  
- Anthropic sandbox-runtime（策略语义参考）：https://github.com/anthropic-experimental/sandbox-runtime  
- 本仓库：`docs/developer/workspace-root.md`、
  [async-subagent-and-terminal.md](async-subagent-and-terminal.md)、
  `docs/developer/file-tool-write-scope.md`
