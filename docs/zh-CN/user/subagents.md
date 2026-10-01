# 子 Agent 设置

[English](../../en/user/subagents.md) | 简体中文

Pointer 可通过 **run_subagent** 将子任务委派给专用 worker（如 **explore** 探索代码库、**coder** 写代码）。

## 用户设置

在 **设置 → 系统设置 → 执行**（或 `settings.json`）中可调整：

| 键 | 界面 | 说明 |
|----|------|------|
| `maxToolRounds` | 本轮 | 主会话本轮工具循环上限（默认 **5000**） |
| `maxSubAgentToolRounds` | 子任务 | 每次子任务内部工具循环上限，与主会话独立（默认 **500**，最高 500） |
| `maxParallelSubAgents` | 工具并行 → 子 Agent | 本会话同时运行的子 Agent 上限，**前台与后台共用**；后台终端也占此额度。前台终端走「通用工具」 |
| `maxParallelToolCalls` | 工具并行 → 通用工具 | 同一轮读工具（含前台终端）同时执行上限 |
| `maxSubAgentSpawnDepth` | （当前无界面） | 嵌套委派最大深度（默认 **2**：主 Agent + 一层子委派）。**每次 spawn 都消耗一层**（`self` fork 也算）；到达上限的 agent 不能再委派 |
| `maxChildrenPerAgent` | （当前无界面） | 单个 agent 同时存活的子 Agent 上限（默认 **8**，范围 **1–32**）。超限的委派直接返回报错，不排队 |

## 内置行为

- 内置 **coder** 已默认允许 **explore** 子 Agent，用于代码库探索
- 助手委派 **explore** / 自身并行任务时默认后台跑：主会话可以继续说话，侧栏该会话保持转圈。点 **停止** 会取消这些后台任务；**立即发送** 或工具行上的 **结束等待** 只结束当前回复（含等待），后台继续。单条后台可用 **结束任务**。后台跑完且主会话当时没有在回复时，会在**同一对话**里再续一轮，把结果汇总给你（界面先出现「后台任务已完成：任务名」）。若结果还要改，助手会接着同一条已完成的子任务往下做，不会把之前的过程丢掉。需要本轮立刻用结果时，助手仍可选择前台等待。子 Agent 里启动的后台终端同样出现在输入框上方的后台任务列表。
- **general** 委派 **computer** 时，**每次**子任务都需用户确认；先前同意过的任务或轮次**不能**自动沿用
- **嵌套**：子 Agent 自己也可以再委派（默认最多两层：主 Agent + 子 Agent + 孙 Agent），执行过程在子任务框里按层级嵌套显示；`self` fork 会标成 `coder (fork)` 之类，便于和真正的 worker 区分。单个 agent 同时存活的子 Agent 数默认上限 8，超出的委派直接报错
- **深层提问**：子 Agent（含深层嵌套）里的 `ask_user` 会显示在**输入框上方**的固定条上（与后台任务条同一位置），不必先展开子任务框；答完后条上短暂显示「已选择 X」再消失

## 自定义 Agent

若你在工作区或扩展中编写自定义 **Lead Agent**（`AGENT.md`），可通过 frontmatter **`allowAgents`** 控制可委派的 worker 列表。格式、参数与 explore 约定见 **[`../developer/pointer-run-subagent.md`](../developer/pointer-run-subagent.md)**。
