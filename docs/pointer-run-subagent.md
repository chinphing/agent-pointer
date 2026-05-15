# run_subagent 与 allowAgents

## 设置项（`settings.json` / 前端设置 API）

| 键 | 类型 | 说明 |
|----|------|------|
| `allowAgents` | `string[]` | 当前主会话 Lead 调用 **`run_subagent`** 时允许的 **worker** `agentId` 列表；需排序去重由存储层归一化。仅这些 id 的元数据会注入系统提示（见 `delegatable_sub_agents_system_block`）。 |
| `maxSubAgentToolRounds` | `number` | **每一次** `run_sub_agent` 内部工具循环的轮次上限，与主会话的 `maxToolRounds` 独立。 |

## 行为摘要

- 子 Agent **禁止**再次调用 `run_subagent`（`allowed_tools` 剔除 + 运行时硬拒绝）。
- Supervisor 模式下，每执行一个子任务消耗外层一轮子任务预算，且该子任务自带内层 `SessionToolBudget`。
