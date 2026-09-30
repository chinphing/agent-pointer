# 子 Agent：`goal` / `context` 与嵌套 `run_subagent`

> 设计稿。实现以仓库代码为准；落地后回更本文。
>
> **回更说明**：§1.2 / §2.2 / §2.3 / §1.3 的节点与深度语义、§9 的 `maxChildrenPerAgent` 已落地（批次 C / D，见 [`subagent-nesting-and-job-scope.md`](subagent-nesting-and-job-scope.md) §2.3、§2.4、§2.7、§5）。要点：`self` fork 在**还有深度预算时**可再委派（到 `maxSubAgentSpawnDepth` 才 leaf）；`general` / `coder` / `explore` 都是嵌套节点，`computer` 仍为 leaf。

## 目标

1. **Instruct 协议**：`goal` + 可选 `context`（**不保留 `instruction`**），宿主组装 system/user（对齐 Hermes 分离 + OpenClaw 任务放 system）。
2. **嵌套委派**：按 **spawn depth** 允许子 worker 再调 `run_subagent`，上限可配置（对齐 Hermes `max_spawn_depth` / OpenClaw `maxSpawnDepth`）。
3. **`self` fork**：general / coder 等 lead 通过 **`run_subagent(agentId="self")`** 做 **隔离执行**——fork 继承父的 `allowAgents`，**还有深度预算时可再委派**，到 `maxSubAgentSpawnDepth` 才成为 leaf（见 §1.2）。

---

## 1.2 `self` fork（隔离执行）

| 角色 | lead（如 `general` / `coder`） | self fork（sub） |
|------|-------------------------------|------------------|
| 对用户 | ✅ 最终回复、澄清 | ❌ 仅 handoff |
| `run_subagent` | ✅ 注册 worker + **`self`** | ✅ **还有深度预算时**（fork 继承父 `allowAgents`）；到 `maxSubAgentSpawnDepth` 才 leaf |
| Skills | lead 有效列表 | **继承** 父快照 |
| 典型用途 | 编排、路由 | 多 skill 步骤、research、附件流水线、独立实现切片 |

Lead 调用：`run_subagent(agentId="self", goal=…, context=…)`。Repo / skill 写入 / 桌面仍直接委派 **coder** / **computer**；coder 广域只读仍用 **explore**。

---

## 1. 参数与类型

### 1.1 `run_subagent` 工具

| 字段 | 必填 | 说明 |
|------|------|------|
| `agentId` | ✓ | worker id（须在**当前 agent** 的 `allowAgents` 中） |
| `goal` | ✓ | 子任务目标 + 完成标准 |
| `context` | ✗ | Lead 已验证事实、依赖摘要、路径、语言等 |
| `title` / `taskId` / `workspaceRoot` / `computerTarget` | ✗ | 与现语义相同 |

#### Goal 编写规范（Lead / Supervisor）

| worker | `goal` 写什么 | 不要写什么 |
|--------|---------------|------------|
| **explore** | 首行 `Scenario: <id>` + 范围 + 完成标准 | 长证据、重复 grep 结果（放 `context`） |
| **computer** | 简短**结果** + **可见完成标准**；`computerTarget` 按需 | 步骤 1/2/3、点击路径、快捷键、工具名（**how** 归 worker） |
| **coder** | 仓库侧结果 + 验收（测试/行为） | 长篇读文件脚本（交给 explore） |

- 协议上 **`goal` 里写编号步骤不会报错**，宿主原样注入子 agent；但默认 **how** 归 worker，Lead 不应擅自写操作剧本。
- **用户明确要求某种做法时**：**`goal`** 仍写结果 + 完成标准；用户的步骤/路径/工具/范围限制放进 **`context`**，标题 **`User-required approach:`**（忠实转述，不添步骤）。worker 在可行时优先尝试；失败或与策略冲突则在 handoff 说明。
- **`context`**：已验证路径、错误原文、用户约束、前序 handoff 摘要。
- 示例（computer，`external`）：

```json
{
  "agentId": "computer",
  "goal": "在本机打开微信，确认主窗口已出现。handoff：微信已打开。",
  "computerTarget": "external"
}
```

提示词落地：`tools/prompts/run_subagent.md`、`agents/general/AGENT.md`；computer worker 的 delegatable `description` 亦提示 outcome-only goal。

### 1.2 `AgentTask`（Supervisor 同形）

```rust
pub struct AgentTask {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub context: String,
    pub depends_on: Vec<String>,
}
```

### 1.3 用户设置

| 键 | 类型 | 默认 | 说明 |
|----|------|------|------|
| `maxSubAgentSpawnDepth` | `number` | **1** | 可发起 `run_subagent` 的最大 **agent 深度**（见 §2） |
| `maxSubAgentToolRounds` | `number` | （已有） | 每一次子循环内工具轮次上限 |

**深度语义（与 Hermes / OpenClaw 对齐）**

| 深度 | 角色 | `run_subagent` |
|------|------|----------------|
| **0** | Lead（主会话 / Supervisor 编排层） | 允许（若 `allowAgents` 非空） |
| **1 … max−1** | Orchestrator 子 agent | 允许（若本 agent 工具策略含 `run_subagent` 且 `allowAgents` 非空） |
| **≥ max** | Leaf 子 agent | **禁止**；工具列表剔除 `run_subagent`，调用时硬拒绝 |

- Hermes 默认 `max_spawn_depth=1` → 仅 parent(0) 可委派，child(1) 为 leaf。
- OpenClaw 默认 `maxSpawnDepth=1`（部分发行说明推荐 2）；Pointer **默认 2**（主 agent + 一层子委派），设置可调 1–4。
- **无上限封顶**（Hermes 风格）：配置值 floor 为 1，不设硬顶（合理范围文档建议 1–4）。

---

## 2. 运行时：深度跟踪

### 2.1 新增上下文字段

```rust
// SubAgentLoopContext / SubagentDelegationContext
pub spawn_depth: u32,           // 当前子 agent 深度（lead 委派出的第一层 = 1）
pub max_spawn_depth: u32,       // 来自 settings，clamp(1, …)
```

- Lead 首次 `run_subagent`：`spawn_depth = 1`。
- 子 agent 内再 `run_subagent`：`child_depth = parent.spawn_depth + 1`。
- 校验：`child_depth <= max_spawn_depth` 才允许 spawn；否则工具结果 `ERROR: spawn depth limit (child would be at depth X, maxSubAgentSpawnDepth=Y)`。

### 2.2 工具白名单（`init_sub_agent_session`）

```
can_spawn = spawn_depth < max_spawn_depth
          && def.allow_agents 非空
          && access_policy 含 run_subagent（或未 deny）

allowed_tools:
  - 若 !can_spawn → 从列表移除 run_subagent
  - `computer` 无 allowAgents → 自然为 leaf；`explore` 的 allowAgents 只有 `explore`（只读，不含写型 worker）
  - coder（allowAgents: [explore]）在 depth=1、max=2 时为 orchestrator；`self` fork 继承父的 allowAgents，同样受深度门控
```

**不**在全局硬剔除子 agent 的 `run_subagent`（修正现有文档「一律禁止嵌套」表述）；改为 **深度 + allowAgents** 门控。

### 2.3 `allowAgents` 作用域

- 嵌套委派校验 **`validate_run_subagent_target(registry, current_agent.allow_agents, agentId)`**。
- 子 agent **不继承** lead 的 `allowAgents`；用**当前 worker** manifest 中的列表。
- 典型：`coder` 子任务可再委派 `explore`；`explore` 只能再委派 `explore`（只读，不含写型 worker）；`computer` 无 `allowAgents` → 不能继续 spawn。`self` fork 继承的是**父的** `allowAgents`（不是 lead 的），同样受深度门控。

### 2.4 Handoff 链

- 嵌套 `run_subagent` **仅阻塞父层**直至子循环结束（现有语义）。
- 孙 agent 的 `content` 作为**父 sub-agent** 的 tool result，由父综合后再 handoff 给 lead。
- 不向 lead 直接透传孙 agent 的 tool 历史。

### 2.5 预算与副作用

| 项 | 行为 |
|----|------|
| `SessionToolBudget` | 每次 `run_sub_agent` 仍新建内层预算（不变） |
| 父层工具轮 | 一次 `run_subagent` 仍消耗父 agent 一轮（不变） |
| `computer_state` | 仍全局共享；嵌套 computer 任务需文档警示 |
| `workspace_root` | 子委派继承当前 sub provider 的 workspace（不变） |

---

## 3. 宿主组装：`goal` / `context`（§1 配套）

### 3.1 新模块 `sub_agent_task_prompt.rs`

- `build_subagent_task_system_blocks(goal, context, extras) -> Vec<String>`
- `build_subagent_initial_user_message(spawn_depth, max_spawn_depth) -> String`

### 3.2 System 块

```
## Assigned task
``` 
{goal}
```

## Lead context          # 仅当 context 非空
…
Delegated from the lead agent — not end-user chat. …

## Spawn depth            # 嵌套时追加
You are sub-agent depth {d}/{max}. …
[orchestrator 段：可 run_subagent 给 allowAgents 内 worker]
[leaf 段：不可再委派]
```

### 3.3 首条 user

短 stub，**不重复 goal**（OpenClaw 风格）。

### 3.4 Supervisor `dependsOn`

前置任务摘要写入子任务 **`context`**，不拼进 `goal`。

---

## 4. UI / Trace

### 4.1 子消息持久化（已实现）

子 agent 多轮 assistant / tool 与 lead 同级写入 `messages` 表，通过 linkage 关联：

| 字段 | 说明 |
|------|------|
| `anchorMessageId` | 父 lead assistant 消息 id |
| `traceId` | `{taskId}:{agentId}`，对应 `agentTrace.id` |
| `taskId` | supervisor / run_subagent 任务 id |
| `spawnDepth` | 嵌套深度 |
| `contextState.included` | 默认 `false`，不进 lead LLM 上下文 |

**`agentTrace` 退化方案**：仅存索引（`id`、`status`、`detail`、`depth`、`collapsed`、`userExpanded`）；详细执行过程从 scoped 子消息读取。`session` 字段仅兼容旧数据。

**恢复**：`init_sub_agent_session` 从 DB 加载同 `anchorMessageId` + `traceId` 的 scoped transcript 重建 `local_history`（P1 续跑）。

| 项 | 改动 |
|----|------|
| `AgentTrace.depth` | 现为固定 `1`；改为实际 `spawn_depth`（1、2、3…） |
| `agent_trace_step_id` | 含 `task_id` + `agent_id` 已够用；深层级注意 traceId 唯一 |
| `SubAgentFrame` / `agentHandlers.ts` | 按 `depth` 缩进（`depth * indent`） |
| `run_subagent` display | summary 用 `title` / `agentId`；可选展示 `(depth N)` |

可选 P2：`parentTraceId` 建树（Hermes desktop 有 parent_id）；首版 **仅 depth 缩进** 即可。

---

## 5. 提示词与文档

### 5.1 工具与 Lead

| 文件 | 要点 |
|------|------|
| `tools/prompts/run_subagent.md` | `goal`/`context`；深度限制；**Goal authoring**（explore / computer / coder）；删除 instruction |
| `agents/coder/prompts/delegation.md` | Goal/Context 模板；何时子 coder 可再委派 explore |
| `agents/explore/AGENT.md` | 只读；`allowAgents: [explore]`（不含写型 worker） |

### 5.2 子 agent system 附录

`sub_agent_header` + **orchestrator / leaf** 段（参考 OpenClaw `buildSubagentSystemPrompt` canSpawn 分支）：

- **Orchestrator**（`spawn_depth < max_spawn_depth` 且可 spawn）：何时再委派、何时自己做、子结果自动回到当前线程。
- **Leaf**：禁止 `run_subagent`；专注当前 `goal`。

### 5.3 Explore 提示词

`instruction` → **`goal`**（Scenario 读 goal 首行；Lead context 在 system 块）。

### 5.4 修订旧文档

- `docs/design/explore-subagent-for-coder.md` §「不可嵌套委派」→ 深度门控表述
- `docs/developer/pointer-run-subagent.md`：新增 `maxSubAgentSpawnDepth`
- `docs/developer/agent-extension-hooks.md` §5.1

---

## 6. 配置落点

| 层 | 文件 |
|----|------|
| Rust 默认 | `models/settings.rs`、`platform_config.rs`、`storage.rs` |
| 前端 | `stores/settings.ts`、`useSettingsDialogForm.ts`、`RuntimeParamsForm.vue`（或等价设置页） |
| Tauri API | `src-tauri` commands 透传（与 `maxSubAgentToolRounds` 同路径） |

设置文案（界面）：**子 Agent 最大嵌套深度** — 「1 = 仅主 agent 可委派；2 = 子 agent 可再委派一层」。

---

## 7. 实现清单（执行顺序）

### Phase A — 协议（goal/context）

- [ ] A1 `AgentTask`：`goal` + `context`
- [ ] A2 `RunSubagentArgs` + `parse_run_subagent_args`（拒绝 `instruction`）
- [ ] A3 `tool_envelope.rs`：`goal`、`context` raw 保留
- [ ] A4 `sub_agent_task_prompt.rs` + `sub_agent_prompt.rs` 组装
- [ ] A5 `supervisor_plan.rs` / `supervisor.rs` / `plan_sync.rs`
- [ ] A6 `run_subagent_delegation.rs` 构造 `AgentTask`

### Phase B — 嵌套深度

- [ ] B1 `max_sub_agent_spawn_depth` 设置链（Rust + 前端）
- [ ] B2 `SubAgentLoopContext` / `SubagentDelegationContext` 传 `spawn_depth`、`max_spawn_depth`
- [ ] B3 `run_subagent_delegation`：spawn 前深度校验；子循环 `spawn_depth + 1`
- [ ] B4 `init_sub_agent_session`：按深度剔除/保留 `run_subagent`；注入 orchestrator/leaf 段
- [ ] B5 `dispatch/subagent.rs`：sub 路径传入正确 `spawn_depth`（从 `SubToolPassConfig` 扩展）
- [ ] B6 `AgentTrace.depth` = 实际深度
- [ ] B7 UI：`agentHandlers` / `SubAgentFrame` 多级缩进

### Phase C — 提示词与文档

- [ ] C1–C6 见 §5
- [ ] C7 本文档与 `explore-subagent-for-coder.md` 回更

### Phase F — 测试

- [ ] F1 `parse_run_subagent_args`：goal 必填；无 instruction
- [ ] F2 深度：max=1 时 depth-1 子 agent 无 `run_subagent`；max=2 时 coder 子可委派 explore
- [ ] F3 超深度 spawn 返回 ERROR
- [ ] F4 `build_subagent_task_*` 块内容
- [ ] F5 `cargo test -p pointer-core -- run_subagent sub_agent supervisor`

---

## 8. 验收标准

1. 仅接受 `goal`（+ 可选 `context`）；`instruction` 报错。
2. 子 agent 首请求：任务在 system **Assigned task**；user 为 stub。
3. `maxSubAgentSpawnDepth=1`（默认）：仅 lead 可 `run_subagent`；任意子 agent 不能再委派。
4. `maxSubAgentSpawnDepth=2`：depth-1 的 **coder** 可 `run_subagent` → **explore**；explore 仍不能 spawn。
5. 孙 agent 结果回到父 sub-agent tool result，不跳过父层。
6. UI trace `depth` 与设置一致；深层子 agent 边框可区分层级。
7. 相关测试全绿。

---

## 9. 非目标（本迭代）

- `instruction` 别名 / 迁移 shim
- 并行 `tasks[]` 批量 spawn（Hermes batch 模式）
- ~~`maxChildrenPerAgent` / 单轮并发子 agent 上限~~：**`maxChildrenPerAgent` 已落地**（默认 8，clamp 1–32，见 [`subagent-nesting-and-job-scope.md`](subagent-nesting-and-job-scope.md) §2.7 / §5）；单轮并发子 agent 上限仍为后续项
- 子 agent 独立模型覆盖（仍为可选后续）

---

## 10. PR 拆分建议

- **PR1**：Phase A + F1/F4（goal/context）
- **PR2**：Phase B + F2/F3 + UI
- **PR3**：Phase C 提示词与文档
