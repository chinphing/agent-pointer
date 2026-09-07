# `run_subagent` Self-Fork 并发设计

## 目标

在不新增模型工具、不增加数组参数的前提下，扩展现有
`run_subagent`：

- `agentId: "self"` 表示 fork 当前 Agent 的执行快照。
- 模型在同一 assistant turn 发出多个独立的
  `run_subagent(self)` tool call。
- 宿主按 `maxParallelSubAgents` 并发执行，并按原 tool call
  顺序 join。
- 删除 `general-worker`，由 `general -> self` 替代其隔离上下文用途。
- `coder -> self` 支持并行实现或调查；现有
  `coder -> explore` 继续保留并保持串行。
- APP 与 Web/server 共用 `pointer-core` 行为。

## 非目标

- 不新增 `fork_tasks` 工具。
- 不新增 `tasks[]` 参数。
- 不让普通跨角色 `run_subagent` 并发。
- 不支持 self fork 递归。
- 不实现后台 handle 或跨进程恢复。
- 不为每个 fork 自动创建 git worktree。
- 不支持并发 Computer 操作。

## 模型协议

现有单目标委派保持不变：

```json
{
  "agentId": "explore",
  "goal": "What: …\nDone when: …",
  "context": "…",
  "taskId": "explore-api"
}
```

新增 self fork：

```json
{
  "agentId": "self",
  "goal": "What: …\nDone when: …",
  "context": "…",
  "taskId": "backend"
}
```

多个 self fork 由模型生成多个独立 tool call，而不是一个批量参数。
每个调用仍只描述一个任务。

`taskId` 未提供时，宿主使用 `tool_call_id` 生成稳定且唯一的
fork id。

## 触发策略

`general` 和 `coder` 仅在同时满足以下条件时生成同轮多个
self fork：

1. 至少两个具有实质工作量的子任务。
2. 子任务没有数据依赖或先后顺序。
3. 子任务不需要向用户单独提问。
4. 子任务不会并发控制同一桌面状态。
5. 涉及写操作时，文件或模块范围应互不重叠。

简单的多个文件读取继续使用普通并行 `file_*`。
未知范围的只读代码探索仍优先使用 `explore`。

## 工具继承

工具继承由工具注册元数据控制，不由模型参数控制。

`ToolEntry` 增加：

```rust
pub inherit_to_subagent: Option<bool>
```

语义：

- `None`：允许继承，保持新工具默认可用。
- `Some(true)`：显式允许。
- `Some(false)`：self fork 不继承。

self fork 的有效工具集合：

```text
父 Agent 当前有效工具快照
∩ 当前平台注册工具
∩ inherit_to_subagent != Some(false)
```

普通跨角色委派仍按目标 Agent manifest 的 allow/deny policy 解析，
不改变现有行为。

### 显式禁止继承

- `run_subagent`
- `memory`
- `skill_import`
- `cron_job`
- `image_generate`
- `video_generate`
- `task_board_abandon`
- 所有 Computer、clipboard、app-access 工具

这些工具涉及递归编排、用户级持久状态、长期任务、final-reply
语义或全局桌面状态。

### 默认允许继承

- `file_read`
- `file_write`
- `file_edit`
- `file_glob`
- `file_grep`
- `file_list`
- `read_lints`
- `terminal`
- `web_search`
- `media_understand`
- `session_search`
- `ask_user`
- `skill_read`
- 除 `task_board_abandon` 外的 task-board 工具

文件写入与 terminal 对 coder fork 是必要能力，不通过禁用解决并发，
而通过任务拆分、实例隔离、路径锁和明确错误处理控制冲突。

## General Agent 调整

删除 `general-worker`：

- 移除 Agent manifest 与相关 prompt。
- 从 `general.allowAgents` 删除 `general-worker`。
- 移除注册、测试、文档及 UI 中的引用。
- General 的隔离任务改为 `run_subagent(agentId="self")`。

self fork 不依赖 `allowAgents`，但仅当当前 Agent 自身拥有
`run_subagent` 时可调用。

## 执行快照

self fork 不从 `AgentRegistry` 查找 `"self"`。宿主从当前执行上下文
构造不可变快照：

- 当前 Agent id、profile、名称和 system prompt。
- 当前有效 skill ids 与 skill prompts。
- 过滤后的有效工具列表。
- 当前 provider/model settings。
- workspace root。
- session user id。
- parent run/conversation/message identity。

每个 child 使用：

- 独立 `local_history`。
- 独立 `AgentInstanceScope`。
- 独立 trace id。
- 独立 child task-board key。
- 独立 token usage accumulator。
- 共享父 CancellationToken。

self fork child 的工具列表不包含 `run_subagent`，因此天然为 leaf，
不消耗现有跨角色 spawn depth。

## 并发调度

Batch planner 仅将同一 wave 中的 self-fork calls 合并到并行 wave。

以下调用继续串行：

- 任何普通跨角色 `run_subagent`。
- `run_subagent(computer)`。
- self fork 与非 self 委派混合时的边界调用。

运行时接入现有 `maxParallelSubAgents`：

- 最小值为 1。
- 同一批超过可用许可时等待 semaphore，而不是丢弃。
- 结果按原 assistant `tool_calls` 顺序提交。
- 单个 child 失败不取消 sibling。
- 父 run 被取消时，所有 child 一起取消。

## 两阶段执行与提交

现有委派在执行过程中直接修改父 `history`、`agent_trace` 和 token
stats，不能直接并发。

改为两阶段：

### 阶段一：并发执行

每个 fork 只写自己的：

- local history
- scoped stream messages
- child trace buffer
- child task board
- usage buffer
- structured handoff result

不得修改父 history 或父 trace vector。

### 阶段二：顺序提交

所有 child 完成后，按原 tool call 索引：

1. 合并 child trace。
2. 合并 token usage。
3. 写入对应 tool result。
4. 更新父 anchor message。
5. 发布最终状态事件。

这样保持 transcript、UI 卡片和模型 tool result 顺序稳定。

## 共享资源与冲突

### 文件

fork 可继承文件写工具。

- 增加 canonical path 级进程内写锁。
- 不同路径可并行。
- 同一路径写入自动串行。
- `file_edit` 保留 old-content 校验；发现内容已变化时明确失败，
  不静默覆盖。

### Terminal

terminal 状态必须从 conversation 粒度扩展到 fork instance：

```text
conversation_id + agent_instance_id + tool_call_id
```

隔离 output routing、pending input 与 abort 状态。
父 cancel 仍清理整个 run 下所有 terminal。

不同 terminal 命令可能修改相同仓库内容，模型提示必须要求仅并发
独立写入范围；宿主无法可靠静态分析任意 shell 命令的副作用。

### Task board

每个 fork 使用唯一 child key：

```text
{parent_store_key}<separator>{task_id}
```

child 不得操作父 board。`task_board_abandon` 由宿主在 child 失败时
执行，不暴露给 self fork。

### 审批

需要用户审批的 inherited 工具继续走现有审批机制。
审批 request id 必须包含 fork instance identity，避免 sibling 覆盖。
用户取消父 run 时，所有 pending approvals 统一取消。

## 错误与可观测性

关键日志：

- batch 中 self fork 数量、并发上限与 wave id。
- child start/end、task id、agent instance id、耗时与状态。
- semaphore 等待时间。
- 文件锁等待与冲突。
- trace/usage commit 结果。
- sibling 失败但批次继续执行。

结果状态至少包含：

- `completed`
- `failed`
- `cancelled`

父 Agent 总能收到每个 tool call 对应的结构化结果。错误不得静默导致
tool call 无结果。

## 测试

### 单元测试

- `agentId=self` 解析与目标解析。
- self 不要求在 `allowAgents` 中。
- 非 self 委派仍执行原有 allow-list 校验。
- 工具继承默认 true、显式 false。
- 禁止继承列表。
- general-worker 已从 registry 删除。
- self fork child 不包含 `run_subagent`。
- batch planner 仅并发 self calls。
- 普通/Computer 委派仍串行。
- `maxParallelSubAgents=1/N` 行为。
- 结果按输入顺序，不按完成顺序。
- sibling failure 不取消其他 child。
- parent cancel 取消所有 child。

### 集成测试

- General 同轮两个 self fork 并发完成并 join。
- Coder 两个只读 fork。
- Coder 两个不同文件写入。
- 同文件写入受路径锁保护。
- fork terminal 输出路由到正确 trace。
- fork child task boards 互不覆盖。
- APP 与 pointer-server 入口使用相同 runtime。

## 兼容性

- 现有 `run_subagent(agentId=<worker>)` 请求不变。
- 不修改持久化数据结构。
- `maxParallelSubAgents` 从仅配置/日志升级为真实运行时限制。
- General-worker 删除后，历史消息中的旧 agent id 仍可展示；
  只是不能再作为新委派目标。
- Windows、macOS、Linux 共用调度逻辑。
- APP 与 Web/server 共用调度逻辑。

