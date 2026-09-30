# 子 Agent 嵌套（多节点）与后台 job 作用域（子树）

> 设计稿。**P1 = §3（job 作用域，本次实施）**；**§2（嵌套）已归档，暂不实施**。
> 落地后回更 `subagent-goal-context-and-nesting.md`、`async-subagent-and-terminal.md`、
> `docs/developer/pointer-run-subagent.md`。

## 0. 决策台账

| 编号 | 决策 | 状态 |
|------|------|------|
| **D-1** | 嵌套节点集合放开为 coder / general / explore 三类 | **已归档**（§2，待启动） |
| **D-2** | 孙 agent 结果只回直接父层 | **已归档**（§2.5，现状已满足，随 §2 一起做回归固化） |
| **D-3** | `job` 的 `list` / `status` / `await` / `cancel` 只作用于 **当前 agent 及其子 agent** 启动的后台任务 | **本次实施**（§3） |

非目标：worktree / 云 VM 隔离；崩溃后恢复 running job；`computer` 后台化；job 工具拆成六动作。

---

## 1. 现状证据（基线）

### 1.1 `job` 无归属维度的根因

| 事实 | 证据 |
|------|------|
| 省略 `jobIds` = **本会话全部 job** | `job_supervisor.rs:1392` `resolve_job_ids` → `inner.by_conversation[conversation_id]` |
| `await` 没有调用者身份 | `job_supervisor.rs:1016` `await_jobs(conversation_id, ids, mode, timeout, cancel, slot_cap)`；调用点 `dispatch/job.rs:88` |
| `any` 模式还按会话级取"已完成未认领" | `job_supervisor.rs:1324` `unclaimed_finished_conversation(inner, conversation_id)` |
| `list` / `status` / `cancel` 同样只有会话维度 | `job_supervisor.rs:944` / `:959` / `:970` |
| job 已记录**直接**发起者（仅用于 push 路由） | `JobRecord.parent_agent_instance_id`（`:106`，`None` = lead）、`bind_parent_agent`（`:692`）、`spawned_by_lead`（`:1544`） |
| 可后台且持有 `job` 工具的 worker 只有 coder | `agent_id_allows_background`（`tools/run_subagent.rs:59-64`）；self fork 被剥 `job`（`tools/job.rs:22` + `self_fork.rs:26`）；explore 白名单只有 `file_*`（`agents/explore/AGENT.md:18-24`） |
| 宿主自身也 await 自己发起的 job（合法，不能被误伤） | `dispatch/terminal.rs:446-457`（`blockUntilMs` 用显式 `job_id` + `AwaitMode::All`） |
| 现有 spawn 侧已携带"直接父 instance id" | `SubagentDelegationContext.parent_agent_instance_id`（`context/loop_ctx.rs:76`）、`BackgroundOwnedSpawn.parent_agent_instance_id`（`run_subagent_delegation.rs:698`）、terminal 的 `parent_agent_instance_id`（`dispatch/terminal.rs:210/383`，来源 `dispatch/mod.rs:208`） |
| instance 没有父指针 | `AgentInstanceScope`（`agent_instance_scope.rs:7-12`）只有 run_id / instance_id / role / conversation_id |

### 1.2 问题 2 的触发链（复现）

1. lead `run_subagent(agentId="coder")` 省略 `background` → 按工具文档即后台（`tools/prompts/run_subagent.md:107`），coder 成为 job **J**（owner = lead）。
2. coder 内 `run_subagent(agentId="explore")` → 同样默认后台，explore 成为 job **K**（owner = coder）。
3. coder 按工具文档指引 `job.await`（`run_subagent.md:120-122`），`jobIds` 省略 → 等待集合 = 会话全部 job，**含 J 自己**。
4. J 只在 coder 本轮结束后才终态 → `mode=all` 必然阻塞到 `timeoutMs`（默认 30 分钟）；`mode=any` 在会话里没有别的"已完成未认领"job 时同样阻塞。期间 coder 占着根槽，界面一直"后台执行中"。

---

## 2. 【已归档】问题 1：嵌套与过程可见性

**状态：不实施。** 归档内容（结论 + 根因链 + 方案）见本文历史版本与以下摘要，启动条件见 §2.6。

- 嵌套**已开启**且默认深度 2（`models/settings.rs:1468`），当前可达链只有 `general → coder → explore`。
- 过程不可见的根因：嵌套 `AgentStep.messageId` = 父 worker 的 scoped 行 id（`dispatch/subagent.rs:200` + `sub_agent.rs:628`）→ 前端 `findMessage` 命中 scoped 行（`src/stores/chat.ts:3144`）→ 孙 trace 挂到**父 scoped 行**的 `agentTrace` → `SubAgentFrame.vue` 模板只渲染工具行、**没有渲染 scoped 行 `agentTrace` 的分支**。
- 方案：`AgentTrace.parentTraceId` 建树 + 前端统一挂 lead 消息 + `SubAgentFrame` 帧内递归渲染（详见上一版 §2.6 A1）。
- 放开多节点需连带处理：`general` 的子形态（self fork 可委派 vs 复活 `general-worker`）、self fork 的"UI-only 深度"hack（`dispatch/subagent.rs:98-105`）、`maxChildrenPerAgent` 扇出护栏。

**启动条件**：D-3 落地并稳定后；或用户明确要求"看到嵌套过程"。

---

## 3. 【P1 实施】`job` 作用域 = 当前 agent 及其子 agent

### 3.1 核心模型：owner chain（发起者路径）

不引入 instance 全局登记表，改用 **job 自带祖先路径**：

```text
chain(lead)      = []                       // lead 不进链，保持跨轮可见
chain(child)     = chain(parent) + [child_instance_id]
job.owner_chain  = chain(发起该 job 的 agent)   // 发起者自己的链（含它自己）
```

**可见性判定**

| 调用者 | 可见集合 |
|--------|----------|
| lead | 本会话**全部** job（root 通配，与现状一致，保证"总有人能收"） |
| 子 agent 实例 C | `job.owner_chain` **包含** C 的 job |

**推论（全部符合 D-3）**

| 场景 | owner_chain | C 可见？ | 说明 |
|------|-------------|----------|------|
| C 自己启动的 job | `[..., C]` | ✅ | "当前 agent 启动的" |
| C 的子 agent 启动的 job | `[..., C, D, ...]` | ✅ | "子 agent 启动的" |
| **C 自己那条 job**（由父发起） | `[..., P]`（**不含 C**） | ❌ | **自等被结构性消除**——这是本方案相对"仅 owner 精确匹配"的关键收益 |
| 父 / 祖启动的 job | `[..., 祖]` | ❌ | 不越权看上级任务 |
| 兄弟 / 无关分支的 job | 不含 C | ❌ | 不串味 |
| 已结束的后代实例启动、仍在跑的 job | 含 C | ✅ | 链式判定不要求链上实例存活 → 天然无孤儿 |

### 3.2 数据模型

```rust
pub struct JobRecord {
    // ...现有字段...
    /// 发起者路径（root 起，含发起者自己）。空 = lead 发起。
    owner_chain: Vec<String>,
    // 删除 parent_agent_instance_id，改为派生：
    //   direct_owner() -> Option<&str> { self.owner_chain.last().map(String::as_str) }
    //   is_lead_owned() -> bool { self.owner_chain.is_empty() }
}

pub enum JobCaller<'a> { Lead, Instance(&'a str) }
impl JobCaller<'_> {
    fn can_see(&self, job: &JobRecord) -> bool {
        match self {
            JobCaller::Lead => true,
            JobCaller::Instance(id) => job.owner_chain.iter().any(|x| x == id),
        }
    }
}
```

### 3.3 API 变更（全部按 `can_see` 过滤）

| 方法 | 位置 | 变更 |
|------|------|------|
| `register(...)` | `:626` | 增加 `owner_chain: Vec<String>` 参数（空 = lead 发起）；删掉 `bind_parent_agent`（`:692`）与其调用点（`run_subagent_delegation.rs:774`、`terminal.rs:400`） |
| `list(conversation_id, include_content, caller)` | `:944` | 增加 `caller`，只列可见 job |
| `status(conversation_id, job_id, caller)` | `:959` | 增加 `caller`；不可见 → `Err("jobId \`<id>\` is not owned by this agent or its sub-agents")` |
| `cancel_ids(conversation_id, ids, caller)` | `:970` | 增加 `caller`；返回 `JobCancelOutcome { cancelled, denied }`；`ids=None` 只取消可见 job；显式 id 越权 → 跳过并单列 `denied[]`（未知 id 仍静默忽略） |
| `await_jobs(...)` | `:1016` | 增加 `caller`；返回 `Result<JobAwaitResult, String>`；`resolve_job_ids`（`:1392`）与 `unclaimed_finished_conversation`（`:1324`）都按可见性过滤；显式 `jobIds` 越权 → `ERROR`（确定性拒绝，不静默过滤；未知 id 保持原静默行为） |
| `try_claim_await` / `snapshot_await` | `:1101` / `:1214` | 透传 `caller` |
| `open_jobs_spawned_by` | `:660` | 改为 `direct_owner() == instance`（语义不变：只列**直接**子任务） |
| `spawned_by_lead` | `:1544` | 改为 `is_lead_owned()`（空闲 push 路由不变） |
| `running_count_for_conversation` / `idle_slots` / `pool_running_roots` / `background_jobs_event` | `:465` / `:458` / `:449` / `:479` | **不改**：界面占用与槽位是会话级，必须看到全部 job |

### 3.4 chain 的传递（改动落点）

`chain` 在 **spawn 时**由发起者往下传；**查询时**只需调用者自己的 instance id（不需要它的链）。

| 落点 | 改动 |
|------|------|
| `SubagentDelegationContext.parent_agent_instance_id` → `issuer_chain: &'a [String]` | `context/loop_ctx.rs:76` |
| lead 路径 | `dispatch/subagent.rs:167` 传 `&[]` |
| sub 路径 | `dispatch/subagent.rs:196` 传 `sub_cfg.agent_chain` |
| `SubToolPassConfig` | 增加 `agent_chain: Vec<String>`，来源 `SubAgentLoopContext.agent_chain`。lead 不新增字段：lead 的链结构上恒为 `[]`，调用点直接传 `&[]` |
| `SubAgentLoopContext` | `context/loop_ctx.rs:45` 附近增加 `agent_chain: &'a [String]`；`execute_owned_subagent` / 串行路径计算 `child_chain = issuer_chain + [child_instance_id]` 后写入（`run_subagent_delegation.rs:507` 一带）；follow-up 用恢复的链 |
| `BackgroundOwnedSpawn` | `parent_agent_instance_id` → `issuer_chain: Vec<String>`（= 发起者链，作 job 的 `owner_chain`）+ `resume_agent_chain: Option<Vec<String>>`（follow-up 恢复链）；来源 `run_subagent_delegation.rs:1396`、`agent_tool_pass/mod.rs:1205` |
| terminal 后台 | `dispatch/terminal.rs:210/383` 的 `parent_agent_instance_id` → `owner_chain: &[String]`；来源 `dispatch/mod.rs:208`（sub → 该 sub 的 chain；lead → `[]`）。内部 `blockUntilMs` 自等的 caller 由 `owner_chain.last()` 派生（空 = lead） |
| follow-up（`worker_followup.rs`） | 续跑的实例需要自己的 chain → 在 scoped 行上持久化（见 3.5） |

### 3.5 持久化与恢复

- `ChatMessage` 已有 `spawn_depth: Option<u32>`（`models/message.rs:359`），同位置增加 `agent_chain: Option<Vec<String>>`，由 `SubMessageLinkage`（`sub_agent_prompt.rs:161`）写入。
- `worker_followup.rs:108-111` 现在从 scoped 行读 `spawn_depth`；同处读回 `agent_chain`，保证续跑实例的 owner_chain 正确（否则续跑期间它 spawn 的 job 会挂错链）。
- job 表是内存态（进程退出即消失），`owner_chain` 无需迁移。

### 3.6 行为对照（改前 / 改后）

| 场景 | 改前 | 改后 |
|------|------|------|
| 后台 coder `job.await`（无 id） | 等自己 → 阻塞到 30min | 等待集合 = 它的子任务；无子任务 → 立即返回 `jobs: []`（不再自等） |
| 后台 coder `job.await`（显式列子 job id） | 正常 | 正常（子 job 可见） |
| lead `job.await`（无 id） | 会话全部 | 会话全部（不变） |
| lead `job.await`（列子 agent 的 job id） | 可认领 | 可认领（lead 通配） |
| 子 agent `job.list` | 看到全部（含父/兄弟） | 只看到自己 + 子孙 |
| 子 agent `job.cancel`（无 id） | 取消整个会话的后台 job（**危险**：会杀掉 lead 与兄弟的任务） | 只取消自己 + 子孙 |
| `terminal.blockUntilMs` 内部 await | 自等（显式 id） | 不变（job 的 owner_chain 含发起者自己，可见） |
| 空闲 push | 仅 lead 发起 | 不变（`is_lead_owned()`） |

### 3.7 决策点

| 编号 | 问题 | 建议 |
|------|------|------|
| **D-B1** | worker 终态时，它（及其子孙）启动的、仍未终态的 job：继续跑 / 自动取消？ | **继续跑**（保持"后台任务可跨轮存活"；链式可见性保证 lead 或任一存活祖先仍能 await）。UI 可标注"发起者已结束" |
| **D-B2** | 子 agent 的 `job.list` / `background_jobs_event` 是否要在 UI 上按 owner 分组显示？ | 建议后续做（本次 UI 不变，仍显示会话全部 job） |
| **D-B3** | 越权显式 `jobIds`：`ERROR` 还是静默过滤？ | `ERROR`（确定性、可测，避免模型误以为任务存在） |

---

## 4. 改动清单（P1）

| # | 文件 | 改动 |
|---|------|------|
| J1 | `crates/pointer-core/src/chat_service/job_supervisor.rs` | `owner_chain` 字段 + `JobCaller`；`register` 带链；删 `bind_parent_agent`；`list`/`status`/`cancel_ids`/`await_jobs`/`resolve_job_ids`/`unclaimed_finished_conversation`/`try_claim_await`/`snapshot_await` 按可见性过滤；`open_jobs_spawned_by`、`spawned_by_lead` 改派生 |
| J2 | `crates/pointer-core/src/chat_service/agent_tool_pass/dispatch/job.rs` | 传入 `caller`（lead → `Lead`；sub → `Instance(sub.instance_scope.agent_instance_id)`）；越权错误文案 |
| J3 | `crates/pointer-core/src/chat_service/agent_tool_pass/dispatch/terminal.rs`（`:210/383/396`）、`dispatch/mod.rs:208` | 传 `owner_chain` 而非 `parent_agent_instance_id`；内部 `blockUntilMs` await 传 `Instance(自己)` |
| J4 | `crates/pointer-core/src/chat_service/run_subagent_delegation.rs`（`:698/750/774/1396`）、`agent_tool_pass/mod.rs:1205` | `BackgroundOwnedSpawn.owner_chain`；spawn 时把子链写入 `SubAgentLoopContext` |
| J5 | `crates/pointer-core/src/chat_service/context/loop_ctx.rs:76`、`agent_tool_pass/dispatch/subagent.rs:167/196` | `parent_agent_instance_id` → `issuer_chain` |
| J6 | `crates/pointer-core/src/models/message.rs:359`、`chat_service/sub_agent_prompt.rs:161`、`chat_service/worker_followup.rs:108` | scoped 行持久化 `agent_chain` 并回读（follow-up 正确性） |
| J7 | `crates/pointer-core/src/tools/prompts/job.md`、`run_subagent.md`、`docs/developer/pointer-run-subagent.md:84`、`docs/design/async-subagent-and-terminal.md` | 文案：`jobIds` 省略 = 你自己 + 你的子 agent 启动的；子 agent 看不到父/兄弟的任务；顺带修掉"仅 self/explore 可后台"等过期表述 |

---

## 5. 测试清单（P1）

| 测试 | 断言 |
|------|------|
| `await_any_excludes_own_parent_spawned_job` | 后台 coder 无 id `await` 立即返回，不等自己那条 job |
| `await_sees_descendant_jobs` | 父 instance 能看到并认领子 instance 的 job |
| `await_rejects_foreign_job_id` | 显式列父/兄弟 job id → `ERROR` |
| `list_scoped_to_subtree` | 子 agent 的 `list` 不含父/兄弟 job |
| `cancel_without_ids_scoped_to_subtree` | 子 agent 的 `cancel` 不动 lead 与兄弟的 job |
| `lead_await_still_sees_all` | lead 行为零变化（回归） |
| `terminal_block_until_ms_waits_own_job` | 宿主自等不被误伤（回归） |
| `owner_chain_derives_direct_owner` | `open_jobs_spawned_by` / `spawned_by_lead` 语义不变 |
| `followup_resumes_chain` | 续跑实例 spawn 的 job 挂在其原链上 |
| 端到端 | `lead → coder(后台) → explore(后台) → coder job.await` 不再卡死；lead 仍能收两条 job 的终稿 |

---

## 6. 分期

| 阶段 | 内容 | 状态 |
|------|------|------|
| **P1** | §3 全部（J1–J7 + §5 测试） | **已实施**（2026-09-30） |
| P2 | §2 嵌套 + UI 树化渲染 | 已归档，待启动 |
| P3 | 扇出护栏 `maxChildrenPerAgent`、取消级联、owner 分组 UI | 后续 |

验收：`cargo test -p pointer-core -- job supervisor subagent` 全绿；手工用例（§5 端到端）不再出现 30 分钟卡死。

---

## 7. 风险

| 风险 | 缓解 |
|------|------|
| 越权过滤导致模型拿不到本该能拿的终稿 | lead 通配 + 链式可见（子孙任务对所有存活祖先可见）；`openBackgroundJobs` 回执已把直接子任务的 id 交给父层 |
| 链在 spawn 时刻固定，若中途"认领关系"变化（转交）则不一致 | 本方案不做转交，链不可变 → 天然一致（这也是相对"实例树 + 转交"的简化点） |
| 持久化新增字段影响历史数据 | `Option<Vec<String>>` 缺省 `None`；`None` 视为"仅自己可见"（退化为保守可见性） |
| 子 agent 看不到父任务后，模型可能反复重试 | `job.md` 明确"看不到 = 不属于你，不要重试"；越权显式 id 返回明确 ERROR |
| `spawned_by_lead` / idle push 语义被改坏 | 用 `is_lead_owned()`（`chain.is_empty()`）保持等价，并有单测回归 |
