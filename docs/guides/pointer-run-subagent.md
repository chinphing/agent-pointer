# run_subagent 与 allowAgents

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

## `general` 委派 `coder` / `computer`（兜底 + 需同意）

内置 **general** lead 可将 **`coder`**、**`computer`** 列入 **`allowAgents`**。政策写在 **`general/AGENT.md`** 的 **Delegation** 段，并在 **`run_subagent`** 工具文档中有 general 专用提醒：

- **专用方案优先**：直接回答、**`skill_*`**、**`web_search`** 等；简单 Q&A 不委派。
- **兜底 + 同意**：`coder` / `computer` 委派前应征询同意（或用户已明确要求代劳）。
- **主动询问（general 可先开口）**：明显需 **coder**（仓库工程）或 **computer**（需截图+键鼠操控可见桌面、其他工具无法驱动）时主动 offer。用户直接选 coder/computer 为 lead 时不适用。

Supervisor 团队模式： **`supervisor/AGENT.md`** 与 **`supervisor_plan.rs`** 规划提示同步同一政策。

## `general` → `coder` 工作目录

general 无 Composer 工作区选择器。委派 **coder** 前应在对话中询问用户**项目绝对路径**：

| 用户回复 | `run_subagent` 参数 | 宿主行为 |
|----------|---------------------|----------|
| 给出路径 | **`workspaceRoot`** = 该绝对路径 | 校验目录存在，写入会话并作为 coder 工作区 |
| 未指定 / 不需要 | 省略 **`workspaceRoot`** | 在 `{app_data_dir}/coder-sandboxes/{conversationId}/` 创建**本会话唯一**临时目录 |

运行时通过 **`workspace_updated`** 流事件同步到前端 `conversation.workspaceRoot`（临时目录会 toast 提示）。实现：`workspace_delegation.rs`、`run_subagent_delegation.rs`。

## 行为摘要

- **`run_subagent` 返回值**：子 Agent 的侦察交付物为 **Markdown**（在工具结果的 **`content`** 字符串中）。`taskId` / `agentId` 等为元数据。子会话内仍按宿主约定使用 **JSON tool envelope**；**`content` 内不是 JSON 报告**。
- 子 Agent **禁止**再次调用 `run_subagent`（`allowed_tools` 剔除 + 运行时硬拒绝）。
- Supervisor 模式下，每执行一个子任务消耗外层一轮子任务预算，且该子任务自带内层 `SessionToolBudget`。

## 内置 worker `explore`

- **用途**：只读代码库侦察（`file` 的 list / glob / grep / read），产出结构化摘要供主会话 **coder** 继续 Plan / Implement；不写文件、不跑 shell、不跑 `read_lints`。
- **与主线程摸底的界限**：coder 在 **Routine workflow** 第 2 步用 `file` 收证据直到能改代码/跑测试即可；**explore** 用于「多轮 `file` 会撑爆主对话」或需要 **instruction 里写死完成形态**（如双向 trace、coverage）的审计式摸底。细则见 coder 提示中的 **Delegating to the `explore` worker**（`crates/pointer-core/src/agents/coder/AGENT.md`）。
- **启用**：在 Lead Agent 的 **`AGENT.md`** 中将 **`explore`** 写入 **`allowAgents`**；未列入则 **`run_subagent`** 目标校验失败。内置 **coder** 已默认配置。
- **提示词**：策略见 `crates/pointer-core/src/agents/explore/AGENT.md`；主 Agent 应在 **`instruction`** 中写清目标、范围、完成标准，并建议附带 **Lead context**（主线程已验证事实），避免子会话重复搜索。
