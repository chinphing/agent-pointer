# 子 Agent 设置

Pointer 可通过 **run_subagent** 将子任务委派给专用 worker（如 **explore** 探索代码库、**coder** 写代码）。

## 用户设置

在 **设置**（或 `settings.json`）中可调整：

| 键 | 说明 |
|----|------|
| `maxSubAgentToolRounds` | 每次子 Agent 内部工具循环的轮次上限，与主会话的 `maxToolRounds` 独立 |
| `maxSubAgentSpawnDepth` | 嵌套委派最大深度（默认 **2**：主 Agent + 一层子委派） |

## 内置行为

- 内置 **coder** 已默认允许 **explore** 子 Agent，用于代码库探索
- **general** 委派 **coder** / **computer** 时，部分场景需用户确认

## 自定义 Agent

若你在工作区或扩展中编写自定义 **Lead Agent**（`AGENT.md`），可通过 frontmatter **`allowAgents`** 控制可委派的 worker 列表。格式、参数与 explore 约定见 **[`../developer/pointer-run-subagent.md`](../developer/pointer-run-subagent.md)**。
