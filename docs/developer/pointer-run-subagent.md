# run_subagent 与 allowAgents

> 设置中的 **`maxSubAgentToolRounds`** / **`maxSubAgentSpawnDepth`** 见 **[`../user/subagents.md`](../user/subagents.md)**。

设计与 Cursor Explore 的对照、Lead→explore 约定等见 **[设计文档：explore 子代理](../design/explore-subagent-for-coder.md)**。

## 配置

### Lead Agent（`AGENT.md` frontmatter）

| 键 | 类型 | 说明 |
|----|------|------|
| `allowAgents` | `string[]` | 该 Lead 调用 **`run_subagent`** 时允许的 **worker** `agentId` 列表；加载时排序去重。仅这些 id 的元数据会注入系统提示（见 `delegatable_sub_agents_system_block`）。内置 **coder** 默认包含 **`explore`**。 |

**`agentId="self"`** 不在 `allowAgents` 中配置：任意拥有 **`run_subagent`** 工具的 agent 均可 fork 自身，用于隔离上下文的 leaf 执行（general、coder 等）。详见 **`run_subagent`** 工具文档与 lead **`AGENT.md`**。

### `agentId` 填自己的 id = `self`

模型在 coder 里写 `agentId: "coder"`、在 general 里写 `agentId: "general"`，语义就是"再来一个我"，这正是 fork；而自身 id 通常不在 `allowAgents` 里，registered 路径必然失败。宿主因此在**工具批次准备阶段**（`agent_tool_pass`，早于 wave 规划）把它改写成 `self`，使编排、执行与 UI 看到同一个目标 id，并记录 `run_subagent: own agent id resolved as self fork`。

只有一种情况不改写：

| 情况 | 行为 | 原因 |
|------|------|------|
| 该 id 已显式写入 `allowAgents` | 保持 registered 路径 | fork（继承当前快照）与新实例语义不同，显式配置优先 |

`workspaceRoot` 可用于 self fork，且优先于父会话工作区。运行时会按所有子代理共用的规则校验它为存在的绝对目录并 canonicalize；无效路径会明确报错，不会静默回退到父工作区。

示例（`crates/pointer-core/src/agents/coder/AGENT.md`）：

```yaml
allowAgents:
  - explore
```

### 用户设置（`settings.json` / 前端设置 API）

| 键 | 类型 | 说明 |
|----|------|------|
| `maxSubAgentToolRounds` | `number` | **每一次** `run_sub_agent` 内部工具循环的轮次上限，与主会话的 `maxToolRounds` 独立（默认 **500**，最高 500）。 |
| `maxSubAgentSpawnDepth` | `number` | 嵌套 `run_subagent` 最大深度（默认 **2**：主 agent + 一层子委派）。 |

## `run_subagent` 参数

| 字段 | 必填 | 说明 |
|------|------|------|
| `agentId` | ✓ | worker id（须在 lead 的 `allowAgents` 中），或保留值 **`self`**（fork 当前 agent；不需 `allowAgents`） |
| `goal` | ✓ | 子任务目标 + 完成标准 |
| `context` | ✗ | 已验证事实、路径、依赖摘要等（含列表文件的 `localPath` / media ref） |
| `background` | ✗ | **`self`** / **`explore`** / **`coder`**：**省略 / `true` = 后台**（立刻 `jobId`；本轮要结果用 **`job.await`**，或等空闲 push）。**`false` = 前台 join**。**`computer`** 始终前台 |

任务由宿主写入子 agent **system**（**Assigned task**）；首条 user 为短 stub，不重复 goal。列表文件路径写在 **`context`**，由 worker planner 在 **`task_board_init`** 时填入 **`work_items_source`**。

## `general` 委派 `coder` / `computer`

**`coder` 可直接委派；`computer` 每次委派前都需经 `ask_user` 取得同意**（针对当前任务；先前同意不能沿用）。政策见 **`general/AGENT.md`** **Delegation** 段及 **`ask_user`** / **`run_subagent`** 工具文档。

| Worker | 同意 | 说明 |
|--------|------|------|
| **`coder`** | 不需要 | 所有仓库源码工作（分析、修改、测试）；skill 写入。下一轮工具 = **`run_subagent(coder)`**，父线程不摸底；用户事实写入 **`context`**，由 **coder**（必要时 **`explore`**）完成。 |
| **`computer`** | 需要（**`ask_user`**） | 本机浏览器/桌面操作。**每个**子任务用 **`ask_user`** 征求同意（禁止仅用正文追问）；仅当**当前用户消息**已明确授权本次桌面操作时可跳过。 |

**留在 general 主线程**：对话、通识、**`skill_*`**、附件；无需读仓库的简单 Q&A。

Supervisor 团队模式： **`supervisor/AGENT.md`** 与 **`supervisor_plan.rs`** 规划提示同步同一政策。

## `general` → `coder` 工作目录

general 无 Composer 工作区选择器。委派 **coder** 前应在对话中询问用户**项目绝对路径**：

| 用户回复 | `run_subagent` 参数 | 宿主行为 |
|----------|---------------------|----------|
| 给出路径 | **`workspaceRoot`** = 该绝对路径（**必填**） | 校验目录存在，作为 coder 工作区 |
| 未指定项目路径 | **`workspaceRoot`** = 当前会话工作区（**必填**，不可省略） | 使用 Composer 已选目录或默认沙箱（见 [workspace-root.md](workspace-root.md)） |

运行时通过 **`workspace_updated`** 流事件同步到前端 `conversation.workspaceRoot`（临时目录会 toast 提示）。子 agent 结束后会再次 emit 以**恢复父 agent 工作区**（后端 thread-local 由 `AgentWorkspaceGuard` 恢复）。实现：`workspace_delegation.rs`、`run_subagent_delegation.rs`。

## 行为摘要

- **一个 spawn 还是拆开**：一个结果一次调用；会撞子循环轮次上限（或已撞上）再拆，A 验收再 B，不要原包重试。见工具文档 **One spawn vs split**。
- **`run_subagent` 返回值**：父模型看到工具结果里的 **`content`**（最后一条 assistant 的 Markdown handoff）。该子 Agent 还有未结束的后台任务时，同一份 JSON 带 **`openBackgroundJobs`**（`jobId`、`status`、`kind`、`title`，无正文），父模型用这些 id 做 `job.status` / `job.await`。过程行在库里但是 lead 上下文外；按子线检索的设计见 [`../design/session-search-scope-extension.md`](../design/session-search-scope-extension.md)（未实现）。`agentId` / `agentName` 为元数据。**不要**把子循环的 `reasoning` 写进这份 JSON：思考只挂在子 Agent 当轮 assistant 上，供下一轮 API 原样带回。历史会话里若已写入 `reasoning` 字段，那是旧行为。
- **`self` fork**：fork 当前 agent 的执行快照（profile、工具、skills、workspace）；独立 `local_history` 与 trace；**leaf**（无 `run_subagent`）；不消耗跨角色 spawn depth。父白名单里的 **`ask_user`** 会继承到 fork（`inherit_to_subagent` 默认允许），子任务可直接澄清，不必回到主会话。
- **并行 wave**：同一 assistant turn 内多个独立 **`self`** 和/或 **`explore`** 可共用 owned-outcome 并行 wave（受 `maxParallelSubAgents` 限制）；**`coder`** / **`computer`** 仍串行（`coder` 可后台，但不进 parallel wave）。前台 wave、串行委派与后台 job **共用**按会话工人池（拆的是等不等，不是两套闸）。依赖任务、重叠写、需用户交互或桌面控制的任务不得并行。
- **后台**：`run_subagent` 对 **`self`** / **`explore`** / **`coder`** **省略 `background` 即后台**（与 `true` 相同），立刻返回 **`{ jobId, status: "running", kind: "subagent" }`**。这条 tool result **一直是句柄**：子任务结束后也不把工人终稿写回去（对齐 Cursor 后台 Task / Codex `spawn_agent`）。终稿只在 **`job.await`** 或（仅 lead 自己开的任务）父轮结束后的空闲合并 push 里交给**直接发起方**。子 Agent 自己开的后台任务不 push 给 lead，也不自动再开一轮，由该子 Agent `job.await`。**`job.list` / `job.status` 只有元数据和 `claimed`，不带 `content`**。显式 **`background: false`** 才前台 join。**`computer`** 始终 join。同回合多路 `coder` 写同一批文件时应用 **`background: false`** 或先 `job.await` 再开下一写。`job.await` `mode=any` 只在 **未认领终态** 醒来：终态进 `jobs[]` 并认领。子智能体内部工具过程不叫醒父模型。`mode=all` 等齐等待集后只返回**尚未 claimed** 的正文。回包里的 **`idleSlots` / `poolRunning`** 是共享工人池（前台 join 也占）；**`runningCount`** 只数后台占用。LLM 往返期间新完成的任务无法打断生成。父轮已 `done` 且 **lead 自己开的** 结果未被 `await` 认领时，宿主在**同一会话**再开一轮（空闲合并 push），把 Completed/Failed 正文交给 lead。终稿带 **`agentInstanceId`**。父模型发现结果不够时，用 **`followupInstanceId`** 再叫同一条已完成工人（新 `jobId`，恢复该 instance 的 scoped 对话）；仍在跑的工人不能 follow-up。取消的旧宿主行保持终态。`terminal` 传 **`blockUntilMs`**（`0` 立刻返回，`N>0` 最多等 N ms）走同一张 job 表，但 **`kind: "terminal"`**：句柄与 `job` 回包都是 shell 命令，不是工人；结束后把完整 stdout JSON 写回原来的 `terminal` 行。子任务 / 后台命令在 JobSupervisor 里跑，不占 `session:{conversation}`。父 `done` 不杀 job，用户停止会取消该会话全部后台 job。**终端**省略 `blockUntilMs` 仍与现网相同（join）。
- **委派 `computer`**：与 Computer lead 发送前相同，阻塞等待 macOS 权限向导（桌面端）与屏幕选择（`computer_monitor_pick_required` → `Composer.beginSubagentMonitorPickFlow`）；单屏自动选定、多屏弹窗、已选屏幕复用。
- 嵌套委派：深度由 **`maxSubAgentSpawnDepth`** 控制（默认 2）。达最大深度的子 agent 为 leaf，无 `run_subagent` 工具。
- Supervisor 模式下，每执行一个子任务消耗外层一轮子任务预算，且该子任务自带内层 `SessionToolBudget`。

## 内置 worker `explore`

- **用途**：只读代码库侦察（`file` 的 list / glob / grep / read），产出结构化摘要供主会话 **coder** 继续 Plan / Implement；不写文件、不跑 shell、不跑 `read_lints`。
- **与主线程摸底的界限**：coder 在 **Routine workflow** 第 2 步用 `file` 收证据直到能改代码/跑测试即可；**explore** 用于「多轮 `file` 会撑爆主对话」或需要在 **`goal`** 里写死完成形态（如双向 trace、coverage）的审计式摸底。细则见 coder 提示中的 **Delegating to the `explore` worker**（`crates/pointer-core/src/agents/coder/AGENT.md`）。
- **启用**：在 Lead Agent 的 **`AGENT.md`** 中将 **`explore`** 写入 **`allowAgents`**；未列入则 **`run_subagent`** 目标校验失败。内置 **coder** 已默认配置。
- **提示词**：策略见 `crates/pointer-core/src/agents/explore/AGENT.md`；主 Agent 用 **`goal`** + **`context`** 写清目标、范围、完成标准与已验证事实。
