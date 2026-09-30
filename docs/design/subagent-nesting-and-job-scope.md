# 子 Agent 嵌套（多节点 + 树化渲染）与后台 job 作用域（子树）

> 状态：**§3 job 作用域已落地并提交**（`3b0782e0`）。**§2 嵌套 = 待评审的新方案**（问题 1 重新梳理版）。
> 实现以仓库代码为准；落地后回更 `subagent-goal-context-and-nesting.md`、`async-subagent-and-terminal.md`。

## 0. 决策台账

| 编号 | 决策 | 状态 |
|------|------|------|
| **D-3** | `job` 的四个动作只作用于**当前 agent 及其子 agent**启动的任务（owner chain） | ✅ 已落地（`3b0782e0`，§3） |
| **D-1** | 嵌套节点集合放开为 **coder / general / explore** 三类 | ✅ 已定（§2.3，按建议值） |
| **D-2** | 孙 agent 结果**只回直接父层** | ✅ 已定（§2.6，现状已满足 + 固化） |
| **D-A1…A6** | general 走 self fork / explore 不派写型 / 父帧显示子任务数 / 根槽提升 / 深度 2 / 扇出 8 | ✅ 已定（§2.11，均按建议值） |
| **D-A7** | self fork 在 UI/日志上与真 worker 区分 | ✅ 已定（纳入 P2） |
| **D-C1…C3** | ask_user 顶部条：挂载位置 / 多 pending 处理 / 2s 计时起点 | ✅ 已落地（§4.6，按建议值） |

---

## 1. 现状证据

### 1.1 嵌套已开启，但只有一条链

| 事实 | 证据 |
|------|------|
| 默认最大嵌套深度 = **2**（不是 1） | `models/settings.rs:1468` `default_max_sub_agent_spawn_depth() -> 2`；平台默认同 `:2407` |
| 深度门控 | `tools/run_subagent.rs:81` `validate_spawn_depth`、`:112` `can_spawn_subagents(depth < max)` |
| 子 agent 能否再委派 | `chat_service/sub_agent_prompt.rs:67` `resolve_subagent_spawn_capability` → `Registered / SelfOnly / None`；`:215-217` Snapshot（self fork）剥掉 `run_subagent` |
| 当前唯一可达嵌套链 | `general.allowAgents=[coder, computer]`（`agents/general/AGENT.md:15`）→ `coder.allowAgents=[explore]`（`agents/coder/AGENT.md:14`）→ `explore` 无 `allowAgents` |
| 目标必须 `role == "worker"` | `tools/run_subagent.rs:263-272` |
| `general-worker` 已于 2026-07 删除，由 `general → self` 替代 | `agents/mod.rs:1613-1621`、`:1623-1660` 两条"必须不存在"断言 |

### 1.2 嵌套过程不可见的根因链（缺口 A）

1. 子路径派发把**父 worker 本轮 scoped 消息 id** 当 `message_id`：`agent_tool_pass/dispatch/subagent.rs:200`（`parent_spawn_depth = sub_cfg.spawn_depth`），而 `sub_cfg.message_id = round_message_id`（`chat_service/sub_agent.rs:628`）。
2. 嵌套 spawn 用它发 `AgentStep`：`chat_service/run_subagent_delegation.rs:507`；`build_subagent_trace`（`:1226`）把同一 id 写进 `trace.anchor_message_id`，`depth = child_spawn_depth`。
3. 前端 `handleAgentStep`（`src/stores/chat/streamHandlers/agentHandlers.ts:21`）→ `findMessage`（`src/stores/chat.ts:3144` 会命中 `scopedStore.findRow`）→ 孙 trace 被挂到**父 worker 的 scoped 行**。
4. `SubAgentFrame.vue` 模板只渲染 `ToolCallRow` / `RawWirePanel` / 压缩标记（`:445-505`），**没有渲染 scoped 行 `agentTrace` 的分支** → 孙 agent 的思考 / 工具行 / 正文无组件读取。

### 1.3 深度语义有两套（缺口 B）

| 路径 | 深度来源 | 结果 |
|------|----------|------|
| 并行 wave（`self` / `explore`） | `dispatch/subagent.rs:98-105`：`max_spawn_depth.max(trace_depth)` | self fork **不吃深度**（注释写明 "UI nesting only"） |
| 普通委派（`run_subagent_delegation.rs:1330` 一带） | `validate_spawn_depth(parent, max)` | 真实消耗深度 |

两条路径对同一个 `self` fork 的深度判定不一致。

### 1.4 实测基线（用户本机会话 `807e1411`，lead = `general`）

| 观察 | 数据 |
|------|------|
| 实际嵌套深度只有 **2** | scoped 行：depth 1 = 3250 行，depth 2 = 9337 行，**无 depth ≥ 3** |
| 各层 agentId | depth 1 = `coder`(1330) + `general`(48)；depth 2 = **`coder` 只有一种** |
| `explore` 从未出现 | depth 1/2 均无 `explore` 行 |
| 深层锚点 | depth 2 的 `anchorMessageId` = depth 1 的 scoped 行 id（不是 lead 消息） |
| 深度拒绝 / self 不可用 | 日志中 `spawn depth limit` = 0 次，`self-fork execution unavailable` = 0 次 |

结论：真实链路是 **`general`(lead) → `coder`(depth 1, 注册) → `coder`(depth 2, self fork)**；
depth 1 的 48 行 `general` 是 **lead 自己的 self fork**。

**为什么看起来是"coder → coder → coder"**：self fork 继承父的 `AgentDef`
（`run_subagent_delegation.rs:487-492` `def_for_trace = &snapshot.def`；`:766` role = `snapshot.def.id`），
`build_subagent_trace` 写 `agent_id = def.id`、`name = def.name` → **fork 与真 coder 同名同 id，UI/日志无法区分**。

**为什么是 `self` 而不是 `explore`**：

1. `agentId: "self"` 在 `validate_run_subagent_target` **提前返回**，不校验 `allowAgents`（`tools/run_subagent.rs:239-241`）；
2. `coder.allowAgents = [explore]`，而 `explore` 只读 → coder 想要"另一个能写代码的 agent"只有 `self` 一条路；
3. 工具文档把 self fork 定位为"独立实现切片"（`tools/prompts/run_subagent.md`），模型照着用。

**为什么最多两层**：self fork 被剥 `run_subagent`（`sub_agent_prompt.rs:221`）+ `allow_agents = []`（`:228`）→ fork 是 leaf。

**顺带发现的路径不一致**：串行委派路径对 `self` 是硬错误
（`run_subagent_delegation.rs:1326-1334` "self-fork execution is not available in this implementation stage"），
而 wave 路径（含**单条** `self`/`explore` 调用，`agent_tool_pass/mod.rs:518`）会执行它；
且 wave 路径对 self 用 `max_spawn_depth.max(trace_depth)` 绕过深度。两条路径语义不一致（§2.4 统一）。

### 1.5 job 作用域已落地（P1，与 §2 的协同基础）

- `JobRecord.owner_chain`（`job_supervisor.rs:109`）+ `JobCaller`（`:154`）+ `visible_to`（`:136`）：lead 看全部；实例看 `owner_chain` 含自己的任务。
- 子实例链 = `issuer_chain + [own instance_id]`（`run_subagent_delegation.rs:504-511`、`:1574-1581`），job 以 `issuer_chain` 注册（`:789`）。
- **由此，嵌套后台任务的结果路由已经自洽**：父实例（及所有祖先、以及 lead）都能 `await` 子任务；后台 worker 不会等到自己（自己那条 job 由父发起，链里不含自己）。

---

## 2. 问题 1 新方案：多节点嵌套 + 树化渲染

### 2.1 目标

| # | 目标 | 判定 |
|---|------|------|
| G1 | **coder / general / explore 三类都可作为嵌套节点**（可被委派、可再委派） | 受深度 + 各自 `allowAgents` 门控 |
| G2 | **任意深度的执行过程在 UI 可见**（思考、工具行、正文、状态） | 树化渲染，帧内递归 |
| G3 | **孙结果只回直接父层**，父层综合后再 handoff | 现状已满足，加回归固化 |
| G4 | **嵌套不放大并发**：深层 worker 不占新根槽 | 沿用 `worker_needs_root_slot(depth<=1)` |
| G5 | **成本有硬上界**：深度上限 + 扇出上限 | 新增扇出护栏 |

非目标：worktree / 云 VM 隔离；跨会话嵌套树；无限深度；崩溃后恢复子线程。

### 2.2 缺口清单（相对目标）

| 缺口 | 内容 | 本方案对应 |
|------|------|-----------|
| A | UI 只渲染一层，深层过程不可见（§1.2） | §2.5 |
| B | `self` fork 是 leaf → `general` 无法作为嵌套节点；且深度有两套语义（§1.3） | §2.3 / §2.4 |
| C | 节点集合被写死成单链（只有 coder→explore） | §2.3 |
| D | 扇出无上限（深度是唯一护栏） | §2.7 |
| E | 嵌套后台任务的交付规则没写进提示词（P1 已给出机制） | §2.6 |

### 2.3 节点矩阵（待确认 D-A1 / D-A2）

| 节点 | role / profile | 现状 allowAgents | 目标 allowAgents（建议） | 可后台 |
|------|----------------|------------------|--------------------------|--------|
| `general`（lead，depth 0） | lead | `coder, computer` | `coder, computer, explore` | — |
| `coder`（depth ≥1） | worker | `explore` | `explore` + general 的子形态（见下） | ✅ 默认后台 |
| `explore`（depth ≥1） | worker | 空 | `explore`（建议；不含 coder，见 D-A2） | ✅ 默认后台 |
| `computer` | worker | 空 | 保持 leaf（桌面权限不可嵌套） | ❌ 始终前台 |

**`general` 如何进入嵌套链——两条路径：**

| | 路径 1（推荐） | 路径 2（备选） |
|---|---|---|
| 做法 | `general` 的子形态 = `agentId: "self"` fork；让 fork **可再委派** | 重新注册 worker 形态的 general（`general-worker`） |
| 改动 | `SelfForkSnapshot` 带 `allow_agents` 快照；Snapshot 不再无条件剥 `run_subagent`；`SubAgentSpawnCapability` 增 `SelfFork` 档 | 新增 `agents/general-worker/AGENT.md`；`general.allowAgents` 加它；删两条"必须不存在"断言 |
| 优点 | 不新增 agent id；与 2026-07 "self fork 替代 general-worker" 决策一致；lead 语义不变 | `agentId` 字面量与用户表述一致；fork 语义不动 |
| 代价 | 推翻"self fork 必为 leaf"（提示词/工具文档要回更）；深度 hack 必须一起修（§2.4） | 与 2026-07 决策相反；要回答"何时用 general-worker、何时用 self" |

> 若选路径 2，`general` 这个 id 与 lead 冲突，只能叫 `general-worker`（或其它 worker 名）。

### 2.4 深度语义统一（缺口 B）

- 统一为 **"每个 agent 实例消耗一层真实深度"**：`child_spawn_depth = parent_spawn_depth + 1`，上限 `maxSubAgentSpawnDepth`。
- 删掉 wave 路径的 `max_spawn_depth.max(trace_depth)` 逃生（`dispatch/subagent.rs:98-105`），两条路径都走 `validate_spawn_depth`。
- `AgentTrace.depth` 与真实深度一致 → UI 缩进直接用它，不再依赖"UI nesting only"的语义。
- 深度上限：默认 **2**（保持），设置项可配 1–4。`general → coder → explore` 恰好 2 层。

### 2.5 UI 树化渲染（缺口 A，核心）

#### 数据

| 字段 | 位置 | 语义 |
|------|------|------|
| `AgentTrace.parent_trace_id: Option<String>`（新增） | `models/message.rs:124` | 父实例的 traceId；lead 直系为 `None` |
| `AgentTrace.depth`（已有） | 同上 `:136` | 真实深度（§2.4 之后） |
| `AgentTrace.anchor_message_id`（已有，**语义不变**） | 同上 | 该层自己的 scoped 行锚点 → 帧取数用 |
| `spawn_depth` / `agent_chain`（已有） | `models/message.rs:359` 一带 | scoped 行持久化，恢复用 |

#### 挂载 / 建树（前端）

**唯一数据源 = trace 选择器**（live 与重载共用）：把「lead 消息的 `agentTrace`（第一层）」与「本会话全部 scoped 行的 `agentTrace`（更深层）」合并，按 `parent_trace_id` 建树。

- 不改 `handleAgentStep` 的落库语义：depth>0 的 trace 仍写在**它自己那一层的 scoped 行**上（现状即如此，DB 里 scoped 行 payload 已有 `agentTrace` 字段），保证重载后树可重建；
- 渲染层用选择器取整棵树，不再依赖"帧是否挂载"或"父行是否渲染"；
- 取数仍走 `trace.anchorMessageId`（`SubAgentFrame.vue:86` `effectiveAnchorId` 已优先它）→ 每层帧能取到自己那一层的 scoped 行。

> 说明：早期草案写的是"depth>0 一律挂到 lead 消息"，会与 scoped 行上的持久化重复。改为上面的选择器方案。

#### 渲染

- `MessageList.vue:1219-1238` 的 host 归并：用同一个选择器取树，按 `parent_trace_id` 组装父子列表。
- `SubAgentFrame.vue` 在工具行之后递归渲染子帧：
  ```html
  <SubAgentFrame v-for="child in childTraces" :key="child.id"
                 :trace="child" :anchor-message-id="effectiveAnchorId" />
  ```
- 缩进：沿用 `marginLeft = (depth-1)*12px`（`:425`），或改为纯帧内缩进避免叠加。
- **fork 标识（D-A7）**：`AgentTrace` 增 `delegation: Option<"self"|"registered">`（后端埋点），前端对 `self` 显示 `coder (fork)` 或独立图标，避免与真 worker 混淆（§1.4）。

#### 折叠 / 统计 / 搜索 / 恢复

| 项 | 规则 |
|----|------|
| 折叠 | 父帧折叠 → 子帧一并隐藏；父帧展开 → 子帧按各自状态渲染 |
| 统计 | 父帧统计**只算本层**（`computeSubAgentStatsFromMessages` 不变）；可选在标题追加"含 N 个子任务"（D-A3） |
| 搜索 | 命中孙层时自动展开其全部祖先帧并定位（建树后按路径回溯） |
| 恢复 | 重载时从 lead 消息的 `agentTrace`（含 `parentTraceId`）用**同一个建树函数**重建整棵树；scoped 行本身已是层级锚 |

#### 备选 A2（改动更小）

不新增 `parentTraceId`，直接在 `SubAgentFrame` 内对 `scopedMessages` 各行的 `agentTrace` 递归渲染。

- 优点：Rust 不动。
- 缺点：树依赖"scoped 行被渲染"的隐式链；`MessageList` 的 host 归并 / 搜索 / 会话导航看不到孙层；折叠与懒加载边界更易错。

> 建议 A1；A2 可作为 A1 落地前的临时可见性补丁。

### 2.6 结果路由与后台任务交付（缺口 E + D-2）

1. **孙正文只回直接父层**（现状已满足）：孙 `content` → 父 sub-agent 的 tool result（`run_subagent_delegation.rs:556` 起）→ 父综合后写自己的 handoff；lead 只拿到父层内容 + `openBackgroundJobs` 元数据。本次加回归测试固化。
2. **嵌套后台任务的交付**（P1 已给机制，需写进提示词）：
   - 子任务 job 的 `owner_chain` 含发起者与其所有祖先 → **直接父层可以 `await`，lead 也可兜底**；
   - 规则：**发起者本轮能等就 `job.await`（显式列 id）**；本轮不等就结束本轮，由父层/lead 后续 `await`（不会被空闲 push 唤醒，因为空闲 push 只针对 lead 自己发起的任务）；
   - 禁止用"省略 `jobIds`"的 await 去等**不是自己子树**的任务（P1 已硬拒）。
3. **提示词**：`run_subagent.md` 补"嵌套结果只回直接父层；要 lead 看到孙的原始证据，父层必须在 handoff 里引用"。

### 2.7 护栏（缺口 D）

| 护栏 | 内容 | 落点 |
|------|------|------|
| 深度 | 默认 2，可配 1–4 | `models/settings.rs`（已有）+ §2.4 统一 |
| **扇出**（新增） | `maxChildrenPerAgent`，默认 8；入口按"本 instance 未终态子任务 + 前台 join 数"计数，超限返回 `ERROR`（确定性，不静默排队） | `run_subagent_delegation` 入口 |
| 取消级联 | 父取消 → 其子树全部取消 | 复用 P1 的 `owner_chain`：`cancel_ids(..., JobCaller::Instance(父))` 即子树 |
| 只读隔离 | `explore` 的 `allowAgents` 不含写型 worker（D-A2） | `agents/explore/AGENT.md` |
| 根槽 | 深层 worker 不占新根槽；祖先根槽已释放时仍在跑的嵌套 job → 提升为根槽（D-A4） | `job_supervisor.rs:379/599` + `run_subagent_delegation` |

最坏规模上界（深度 2 + 扇出 8）：1 + 8 + 64 = 73 个实例，可控。

### 2.8 改动清单

| # | 文件 | 改动 |
|---|------|------|
| N1 | `chat_service/self_fork.rs` | `SelfForkSnapshot` 增 `allow_agents`（路径 1） |
| N2 | `chat_service/sub_agent_prompt.rs:214-222` | Snapshot 不再无条件剥 `run_subagent`；`allow_agents` 取快照；capability 增 `SelfFork` |
| N3 | `chat_service/sub_agent_task_prompt.rs:41` | `build_subagent_spawn_depth_block` 补 `SelfFork` 文案 |
| N4 | `agent_tool_pass/dispatch/subagent.rs:35,98-105` | 删 `max(trace_depth)` 逃生，self fork 走真实深度 |
| N5 | `agents/{general,coder,explore}/AGENT.md` | `allowAgents` 矩阵（§2.3） |
| N6 | `chat_service/run_subagent_delegation.rs:1226` | `build_subagent_trace` 写 `parent_trace_id` |
| N7 | `models/message.rs:124`、`models/stream_event.rs` | `AgentTrace.parent_trace_id` + 事件透传 |
| N8 | `src/stores/chat/streamHandlers/agentHandlers.ts:20-40` | depth>0 挂 lead 消息 + 建树 |
| N9 | `src/components/chat/MessageList.vue:1219-1238` | host 归并按 `parent_trace_id` 组装父子 |
| N10 | `src/components/chat/message/assistant/SubAgentFrame.vue:445-505` | 帧内递归渲染子帧 |
| N11 | 新设置项 `maxChildrenPerAgent` | `models/settings.rs` + 前端表单 + 入口校验 |
| N12 | `tools/prompts/run_subagent.md`、各 `AGENT.md`、两个设计稿 | 提示词与文档回更 |

### 2.9 测试清单

| 测试 | 断言 |
|------|------|
| `nested_chain_allows_coder_general_explore` | 按最终矩阵，三类节点在 depth ≤ 2 内可被委派 |
| `spawn_depth_limit_rejects_at_max` | 超深度返回 ERROR（扩展既有用例） |
| `self_fork_consumes_real_depth` | fork 链不再有 `max(trace_depth)` 逃生 |
| `self_fork_can_spawn_when_allow_agents_non_empty` | 路径 1 生效 |
| `grandchild_content_not_visible_to_lead` | D-2 固化：孙正文不进 lead 的 tool result |
| `grandchild_trace_has_parent_trace_id` | Rust 埋点正确 |
| `fanout_limit_rejects_extra_spawn` | 扇出上限 |
| `cancel_cascades_to_subtree` | 取消级联（复用 owner_chain） |
| 前端 vitest | `agentHandlers` 挂载到 lead 消息；`MessageList` 建树；`SubAgentFrame` 递归渲染；折叠/搜索定位 |

### 2.10 分期

| 阶段 | 内容 | 依赖 |
|------|------|------|
| **P2** | UI 树化渲染（N6–N10 + 前端测试） | 无（可先做，只让**现有**嵌套可见） |
| **P3** | 节点矩阵 + fork 可委派 + 深度统一（N1–N5） | D-A1 / D-A2 |
| **P4** | 护栏与级联（N11、§2.7） | D-A4 |
| **P5** | 文档回更（N12） | — |

> P2 与 P3 解耦：先做 P2 就能让当前 `general → coder → explore` 的过程可见；P3 才放开节点集合。

### 2.11 待确认

| 编号 | 问题 | 建议 |
|------|------|------|
| **D-A1** | `general` 参与嵌套：路径 1（self fork 可委派）还是路径 2（复活 `general-worker`） | 路径 1 |
| **D-A2** | `explore.allowAgents` 是否允许写型 `coder` | 不允许（保只读隔离）；`explore → explore` 可 |
| **D-A3** | 父帧标题是否显示"含 N 个子任务" | 显示，但统计只算本层 |
| **D-A4** | 祖先根槽已释放时仍在跑的嵌套 job | 提升为根槽（不丢任务） |
| **D-A5** | 深度上限 | 维持 2（可配 1–4） |
| **D-A6** | 扇出上限默认值 | 8 |
| **D-A7** | self fork 是否要在 UI/日志上与"真 worker"区分（如显示 `coder (fork)` 或独立图标 + `parentTraceId`） | 区分。否则 fork 与真 coder 同名同 id（§1.4），排查与统计都会混 |

### 2.12 风险

| 风险 | 缓解 |
|------|------|
| 放开 `coder` 嵌套后同一 checkout 多写者 | coder 保持串行 + 提示词建议 `background: false`；worktree 隔离留后续 |
| `self` fork 不再 leaf，提示词/文档与实际不符 | P3 同批回更；`SubAgentSpawnCapability::SelfFork` 文案明确"可委派但不进并行 wave" |
| 树化渲染影响既有折叠/统计/搜索 | 建树函数单测 + 折叠态快照测试；搜索命中后按祖先路径展开 |
| 深层嵌套 token/时间成本 | 深度 + 扇出双护栏；`idleSlots` 语义不变（共享池） |
| 只读隔离被 `explore → coder` 绕过 | D-A2 默认禁止；若确需放开，需在策略层显式声明"委派出去的写权限" |

---

## 3. 【已落地】job 作用域 = 当前 agent 及其子 agent

提交：`3b0782e0`（`feat(job): scope background jobs to the caller's agent subtree`）。

| 项 | 实现 |
|----|------|
| 模型 | `JobRecord.owner_chain: Vec<String>`（`job_supervisor.rs:109`）；`chain(lead)=[]`，`chain(child)=chain(parent)+[child_id]`，job 以 `issuer_chain` 注册 |
| 可见性 | `JobCaller { Lead, Instance }`（`:154`）+ `visible_to`（`:136`）：lead 全量；实例 = `owner_chain` 含自己 |
| API | `list/status/cancel_ids/await_jobs` 全部按可见性过滤；`resolve_job_ids`（`:1549`）/`unclaimed_finished_conversation` 同步；越权显式 `jobIds` → `ERROR`；`cancel` 返回 `JobCancelOutcome{cancelled, denied}` |
| 不变 | `running_count_for_conversation` / `idle_slots` / `pool_running_roots` / `background_jobs_event`（会话级，UI 与槽位） |
| 宿主自等 | `terminal_self_caller(owner_chain)` = `owner_chain.last()`，`blockUntilMs` 不受影响 |
| 持久化 | scoped 行 `agentChain`；`worker_followup` 回读（缺省退化为自链） |
| 复测 | lib 全量 1654 passed / 0 failed；`job/supervisor/subagent` 过滤 119 passed |

---

## 4. 【已落地】ask_user 显式外显（顶部条）

### 4.1 问题

嵌套后 `ask_user` 卡片挂在子 agent 的 scoped 工具行上；回合折叠 / 帧折叠 / 帧未挂载 / 嵌套过深时，用户看不到提问入口（现有代码是"为了让嵌套 ask_user 露出来"而**强行保留帧与宿主行**，副作用大且仍然会被深度藏住）。

### 4.2 现状机制（要剔除的部分）

| 位置 | 作用 |
|------|------|
| `src/lib/messageTooling.ts:9` `isInteractiveToolCall` | `pending_approval` **或** `ask_user(pending/running)` |
| `src/lib/messageTooling.ts:56` `hostNeedsCollapsedSubAgentFrames` → `AssistantModelMessage.vue:148-153` | 折叠回合仍挂载子帧，只为露出嵌套 ask_user |
| `src/lib/messageTooling.ts:72` `agentTraceNeedsCollapsedSurface` → `src/lib/messageListLayout.ts:612` | 同上 |
| `src/components/chat/message/assistant/SubAgentFrame.vue:222-228, 493-505` | 折叠帧下方保留交互卡片 |
| `src/components/chat/message/assistant/SubAgentFrameHost.vue:97` `interactiveBlocksStub` | 有待答卡片时不降级为 stub |
| `src/lib/conversationTurns.ts:19` `isInteractive` | 回合进行中仍保留 pending ask_user 可见 |

**剔除范围（只针对 ask_user，`pending_approval` 行为不变）**：
`isInteractiveToolCall` 拆为 `isPendingApprovalToolCall`（保留原语义）与 `isPendingAskUserToolCall`（仅供顶部条使用）；上表各处不再因 **ask_user** 保留帧 / 行 / stub。

### 4.3 新表面：对话区顶部固定条

| 项 | 设计 |
|----|------|
| 组件 | `src/components/chat/AskUserBanner.vue`（复用 `AskUserOptions.vue` 的选项卡片；Banner 只负责定位 toolCall 与残留计时） |
| 挂载 | `src/components/chat/ChatView.vue` 主区 `<MessageList>` 之上（对话区顶部，sticky，移动端同位置）。备选：`<Composer>` 之上（一个挂载点的差别） |
| 数据源（**与帧是否渲染无关**） | ① `chat.current.messages[].toolCalls` 里的 `ask_user`(pending/running)；② **scoped 行**（深层子 agent）——需 `useConversationScopedStore()` 提供"按会话取全部行"的只读访问器（若缺则补） |
| 多 pending | 队列：显示最早的一条，答完自动切下一条；可提示"还有 N 条" |
| 提交后 | 进入"已选择 X"态并**保留 2 秒**再消失（本地 linger 状态，避免 tool call 立刻变 success 导致提前卸载）；2s 内若出现新 pending 则替换 |
| 交互 | 与现有卡片一致：单选点击即提交；多选保留确认按钮；"其他"输入保留 |
| IM | 不影响：IM 走 `im_ask_user` 推送 |

### 4.4 实施顺序（同一批内先加后删）

1. 先加顶部条 + 数据源（含 scoped 全量读取口），用测试锁定"深层 scoped 行的 pending ask_user 能被取到"；
2. 再删 §4.2 的 ask_user 分支；
3. 最后清理不再被引用的辅助函数与测试。

### 4.5 决策点

| 编号 | 问题 | 默认 |
|------|------|------|
| **D-C1** | 挂载位置：对话区顶部 / 输入框上方 | 对话区顶部 |
| **D-C2** | 多个 pending：队列逐个答 / 只显示最早一条 + 剩余数量提示 | 队列逐个答（并显示剩余数量） |
| **D-C3** | 2s 从提交时开始计 | 是 |

### 4.6 落地记录（批次 A）

| 项 | 实现 |
|----|------|
| 新组件 | `src/components/chat/AskUserBanner.vue`（复用 `AskUserOptions.vue`）；`AskUserOptions` 新增 `submitted` 事件，2s 从提交成功起算（D-C3） |
| 队列逻辑（纯函数） | `src/lib/askUserBanner.ts`：`pendingAskUserToolCalls` / `resolveAskUserBannerView` / `createAskUserLinger`；`ASK_USER_BANNER_LINGER_MS = 2000` |
| 挂载 | `src/components/chat/ChatView.vue`：消息区容器改为 `flex h-full min-h-0 flex-col`，条挂在 `<MessageList>` 之上（D-C1）。消息区自己滚动，条因此始终钉在对话区顶部（未用 CSS `sticky`，效果等同） |
| 数据源 | 当前会话 lead `messages[].toolCalls` + `useConversationScopedStore().listRows(convId)`（**深层 scoped 行**，不依赖任何帧挂载）；响应式依赖 = `getMembershipSignal`（spawn 增删）+ `getLiveSignal`（行/工具状态变化） |
| 多 pending | D-C2：队列逐个答，条上提示「还有 N 条待回答」；linger 期间若出现**提交时不在队列里的**新 pending，则立即替换确认态 |
| 旧逻辑剔除 | `messageTooling.ts`：`isInteractiveToolCall` 拆为 `isPendingApprovalToolCall` + `isPendingAskUserToolCall`；`agentTraceNeedsCollapsedSurface` 只按 `status === 'running'`；`SubAgentFrame.vue` 折叠区只保留 approval 卡片；`SubAgentFrameHost.vue` `interactiveBlocksStub` → `approvalBlocksStub`（含 frame memo 依赖）；`messageListLayout.ts` hidden-process 计数改用 `isCollapsedSurfaceToolCall` |
| 未改（有意） | `conversationTurns.ts` 的 `isInteractive` 是通用保留钩子（approval / running 仍需），ask_user 耦合只在 `messageListLayout.entryIsInteractive`；`ToolCallRow.vue` 仍在工具行自身渲染处显示 ask_user 卡片（展开帧中会与顶部条并存，属既有渲染路径，本次未动） |
| 测试 | 新增 `src/lib/askUserBanner.test.ts`（含深层 scoped 行读取）、`src/components/chat/AskUserBanner.test.ts`（happy-dom：深层 pending 显示 / 提交后 2s 隐藏 / 队列切换）；更新 `messageTooling.test.ts`、`messageListLayout.test.ts` 的 ask_user 保留断言。`npx vitest run` → 122 files / 976 passed；`npx vue-tsc --noEmit` → 干净 |

---

## 5. 实施队列

| 批次 | 内容 | 依赖 |
|------|------|------|
| **A** | §4 ask_user 顶部条 + 剔除旧外显逻辑 | ✅ 已落地（`e0cec02c`） |
| **B** | §2.5 UI 树化渲染 + §2.11 D-A7 fork 标识（含 `parent_trace_id` / `delegation` 埋点） | 无 |
| **C** | §2.3 节点矩阵 + §2.4 深度语义统一（含 self fork 可委派） | D-A1/A2 |
| **D** | §2.7 扇出护栏 `maxChildrenPerAgent` + 取消级联 + §2.7 根槽提升 | 无 |
| **E** | §6 子 agent `content` 显示 | B（复用同一渲染骨架） |

每批次独立提交；A/B/E 为前端为主，C/D 为后端为主。

---

## 6. 批次 E：子 agent 的 `content` 显示（新需求）

### 6.1 现状

`SubAgentFrame.vue:146` 已经用 `latestSubAgentBodyModelFromSpawnRows`（`src/lib/subAgentMessages.ts:305`）算出 body（其注释写明 "aggregate tools across rounds; **keep latest assistant text**"），但**模板只用它的 `toolCalls`（`:203`）与 raw 面板**，`body.content` **从未渲染** → 子 agent 写的中间结论与最终 handoff 在 UI 上不可见（只有统计摘要行）。

### 6.2 目标

在子 agent 帧内显示它的 assistant 文本：**每轮的中间结论**（与工具行按时间交错）+ **最终 handoff**。

### 6.3 方案

| 项 | 设计 |
|----|------|
| 展开帧 | 按轮次交错渲染「该轮 `content` → 该轮工具行」；轮次切分用现成的 `buildSubAgentBodyModelsFromScoped`（`subAgentMessages.ts:183`），压缩标记走 `splitSubAgentBodyModelsForCompression`（`:221`） |
| 文本渲染 | **不复用 `AgentMessageBody`**（它带 lead 专属 chrome：复制按钮 / 媒体 / 任务板 / 平台余额）；抽一个轻量 `SubAgentContentBlock.vue`，复用 `parseMarkdown`（`lib/markdownConfig`）+ `useThrottledMarkdown`（流式节流）+ 代码复制 composable |
| 折叠帧 | 统计摘要行之后追加**一行内容预览**（最新 `content` 的首个非空行，截断）；最终 handoff 长文本只在展开态全文 |
| 搜索 | 帧内 `content` 需能被会话内搜索命中（与现有 tool call 搜索一致） |
| 依赖 | 复用批次 B 的渲染骨架（递归帧 + 选择器），B 之后做，避免两次改同一模板 |

### 6.4 决策点

| 编号 | 问题 | 建议 |
|------|------|------|
| **D-E1** | 折叠帧是否显示一行内容预览 | 显示 |
| **D-E2** | 显示全部轮次 content 还是只显示最终 content | 全部轮次，按时间交错 |
| **D-E3** | 是否复用 `AgentMessageBody` | 不复用，抽轻量块 |
