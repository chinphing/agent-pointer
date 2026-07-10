# run_subagent 与 allowAgents

> 设置中的 **`maxSubAgentToolRounds`** / **`maxSubAgentSpawnDepth`** 见 **[`../user/subagents.md`](../user/subagents.md)**。

设计与 Cursor Explore 的对照、Lead→explore 约定等见 **[设计文档：explore 子代理](../design/explore-subagent-for-coder.md)**。

## 配置

### Lead Agent（`AGENT.md` frontmatter）

| 键 | 类型 | 说明 |
|----|------|------|
| `allowAgents` | `string[]` | 该 Lead 调用 **`run_subagent`** 时允许的 **worker** `agentId` 列表；加载时排序去重。仅这些 id 的元数据会注入系统提示（见 `delegatable_sub_agents_system_block`）。内置 **coder** 默认包含 **`explore`**。 |

示例（`crates/pointer-core/src/agents/coder/AGENT.md`）：

```yaml
allowAgents:
  - explore
```

### 用户设置（`settings.json` / 前端设置 API）

| 键 | 类型 | 说明 |
|----|------|------|
| `maxSubAgentToolRounds` | `number` | **每一次** `run_sub_agent` 内部工具循环的轮次上限，与主会话的 `maxToolRounds` 独立。 |
| `maxSubAgentSpawnDepth` | `number` | 嵌套 `run_subagent` 最大深度（默认 **2**：主 agent + 一层子委派）。 |

## `run_subagent` 参数

| 字段 | 必填 | 说明 |
|------|------|------|
| `agentId` | ✓ | worker id（须在 lead 的 `allowAgents` 中） |
| `goal` | ✓ | 子任务目标 + 完成标准 |
| `context` | ✗ | 已验证事实、路径、依赖摘要等（含列表文件的 `localPath` / media ref） |

任务由宿主写入子 agent **system**（**Assigned task**）；首条 user 为短 stub，不重复 goal。列表文件路径写在 **`context`**，由 worker planner 在 **`task_board_init`** 时填入 **`work_items_source`**。

## `general` 委派 `coder` / `computer`

**`coder` 可直接委派；`computer` 每次委派前都需用户同意**（针对当前任务；先前同意不能沿用）。政策见 **`general/AGENT.md`** **Delegation** 段及 **`run_subagent`** 工具文档。

| Worker | 同意 | 说明 |
|--------|------|------|
| **`coder`** | 不需要 | 所有仓库源码工作（分析、修改、测试）；skill 写入。下一轮工具 = **`run_subagent(coder)`**，父线程不摸底；用户事实写入 **`context`**，由 **coder**（必要时 **`explore`**）完成。 |
| **`computer`** | 需要 | 本机浏览器/桌面操作。**每个**子任务先 offer 代操并取得同意；仅当**当前用户消息**明确授权本次桌面操作时可省略单独追问。 |

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

- **`run_subagent` 返回值**：子 Agent 的侦察交付物为 **Markdown**（在工具结果的 **`content`** 字符串中）。`taskId` / `agentId` 等为元数据。子会话内仍按宿主约定使用 **JSON tool envelope**；**`content` 内不是 JSON 报告**。
- **委派 `computer`**：与 Computer lead 发送前相同，阻塞等待 macOS 权限向导（桌面端）与屏幕选择（`computer_monitor_pick_required` → `Composer.beginSubagentMonitorPickFlow`）；单屏自动选定、多屏弹窗、已选屏幕复用。
- 嵌套委派：深度由 **`maxSubAgentSpawnDepth`** 控制（默认 2）。达最大深度的子 agent 为 leaf，无 `run_subagent` 工具。
- Supervisor 模式下，每执行一个子任务消耗外层一轮子任务预算，且该子任务自带内层 `SessionToolBudget`。

## 内置 worker `explore`

- **用途**：只读代码库侦察（`file` 的 list / glob / grep / read），产出结构化摘要供主会话 **coder** 继续 Plan / Implement；不写文件、不跑 shell、不跑 `read_lints`。
- **与主线程摸底的界限**：coder 在 **Routine workflow** 第 2 步用 `file` 收证据直到能改代码/跑测试即可；**explore** 用于「多轮 `file` 会撑爆主对话」或需要在 **`goal`** 里写死完成形态（如双向 trace、coverage）的审计式摸底。细则见 coder 提示中的 **Delegating to the `explore` worker**（`crates/pointer-core/src/agents/coder/AGENT.md`）。
- **启用**：在 Lead Agent 的 **`AGENT.md`** 中将 **`explore`** 写入 **`allowAgents`**；未列入则 **`run_subagent`** 目标校验失败。内置 **coder** 已默认配置。
- **提示词**：策略见 `crates/pointer-core/src/agents/explore/AGENT.md`；主 Agent 用 **`goal`** + **`context`** 写清目标、范围、完成标准与已验证事实。
