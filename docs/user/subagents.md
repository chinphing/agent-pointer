# 子 Agent 设置

Pointer 可通过 **run_subagent** 将子任务委派给专用 worker（如 **explore** 探索代码库、**coder** 写代码）。

## 用户设置

在 **设置 → 系统设置 → 执行 → 轮次**（或 `settings.json`）中可调整：

| 键 | 界面 | 说明 |
|----|------|------|
| `maxToolRounds` | 本轮 | 主会话本轮工具循环上限（默认 **5000**） |
| `maxSubAgentToolRounds` | 子任务 | 每次子任务内部工具循环上限，与主会话独立（默认 **500**，最高 500） |
| `maxSubAgentSpawnDepth` | （当前无界面） | 嵌套委派最大深度（默认 **2**：主 Agent + 一层子委派） |

## 内置行为

- 内置 **coder** 已默认允许 **explore** 子 Agent，用于代码库探索
- 助手可以把部分 **explore** / 自身并行任务放到后台跑：主会话可以继续说话，侧栏该会话保持转圈；点停止会取消这些后台任务
- **general** 委派 **computer** 时，**每次**子任务都需用户确认；先前同意过的任务或轮次**不能**自动沿用

## 自定义 Agent

若你在工作区或扩展中编写自定义 **Lead Agent**（`AGENT.md`），可通过 frontmatter **`allowAgents`** 控制可委派的 worker 列表。格式、参数与 explore 约定见 **[`../developer/pointer-run-subagent.md`](../developer/pointer-run-subagent.md)**。
