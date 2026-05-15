# run_subagent 与 allowAgents

设计与 Cursor Explore 的对照、Lead→explore 约定等见 **[设计文档：explore 子代理](../design/explore-subagent-for-coder.md)**。

## 设置项（`settings.json` / 前端设置 API）

| 键 | 类型 | 说明 |
|----|------|------|
| `allowAgents` | `string[]` | 当前主会话 Lead 调用 **`run_subagent`** 时允许的 **worker** `agentId` 列表；需排序去重由存储层归一化。仅这些 id 的元数据会注入系统提示（见 `delegatable_sub_agents_system_block`）。 |
| `maxSubAgentToolRounds` | `number` | **每一次** `run_sub_agent` 内部工具循环的轮次上限，与主会话的 `maxToolRounds` 独立。 |

## 行为摘要

- 子 Agent **禁止**再次调用 `run_subagent`（`allowed_tools` 剔除 + 运行时硬拒绝）。
- Supervisor 模式下，每执行一个子任务消耗外层一轮子任务预算，且该子任务自带内层 `SessionToolBudget`。

## 内置 worker `explore`

- **用途**：只读代码库侦察（`file` 的 list / glob / grep / read），产出结构化摘要供主会话 **coder** 继续 Plan / Implement；不写文件、不跑 shell、不跑 `read_lints`。
- **启用**：将 **`explore`** 加入 **`allowAgents`**（与 `coder` 等并列）；未列入则 **`run_subagent`** 目标校验失败。
- **提示词**：策略见 `crates/pointer-core/src/agents/explore/AGENT.md`；主 Agent 应在 **`instruction`** 中写清目标、范围、完成标准，并建议附带 **Lead context**（主线程已验证事实），避免子会话重复搜索。
