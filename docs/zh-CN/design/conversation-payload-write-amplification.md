# 会话 payload 写放大审计（`agentTrace` 与同类字段）

> 审计日期：2026-10-04
> 审计方式：**只读**查询本机 `~/Library/Application Support/PointerApp/conversations.db`，未写入任何数据
> 样本会话：`c18c15d2-97ae-4d4a-9984-2db2d58e3a06`（9112 行 = lead 2633 + scoped 6479）
> 单位：**字符数**（SQLite `length()` 对 TEXT 返回字符数；中文实际 UTF-8 字节约为其 3 倍）
> 相关文档：`subagent-nesting-and-job-scope.md`（子 agent 归属渲染，同一份 `agentTrace` 的前端一面）
> 本文含**改造方案**：§10「agent 存储双层化」（提案 + 评估 + 分期草案 + 未决问题）。

---

## 0. 结论摘要

1. **`agentTrace` 在体量上不是问题**：6,819 字符 / 41,276,775 字符 = **0.017%**。
2. 它是**写放大**问题：每次写入都要 `UPDATE` 整行 payload 并触发 FTS 重写，实测放大 **≈ 13–23 倍（均值 ~17×）**。
3. 放大的主体**与 trace 无关**：锚行里 `reasoning` 占 55–69%、`toolCalls` 占 18–34%，而 `content` 只有 45–134 字符。
4. 真正的体量在别处：`content` 43.4% + `reasoning` 21.7% + `toolCalls` 18.8% = **84%**。
5. 两个历史坑已被堵住：`imagesBase64` = 0、`toolRawOutput` = 0（未落库）。
6. **要治的是"写后改 = 整行重写"这个结构**，不是 `agentTrace` 这个字段。

---

## 1. 存储结构（现状）

- `messages` 表：`(id, conversation_id, message_id, role, content, payload, created_at_ms, position, is_system_generated, context_included, tool_name, agent_instance_id, is_scoped, is_milestone)`；`payload` 是**整条 `ChatMessage` 的 JSON**（TEXT 列）。
- 索引：`idx_messages_conv_pos(conversation_id, position)`、`idx_messages_conv_anchor`、`idx_messages_conv_included_pos`。
- **FTS 触发器三个**：`messages_ai`（AFTER INSERT）、`messages_ad`（AFTER DELETE）、**`messages_au`（AFTER UPDATE）**。
- 因此：**任何 `UPDATE messages` 都会重写该行的 FTS 条目**（含 `content` 全文），哪怕只改一个整数列。

### 1.1 `agentTrace` 的写入路径

```
终态（completed/failed/cancelled/error）
  → sync_anchor_agent_trace_index（内存整数组 to_vec + 整数组序列化）
  → schedule_anchor_agent_trace_persist（静态 map 覆盖合并，单后台线程，400ms）
  → flush_pending_anchor_agent_traces
  → patch_agent_trace_json:
       UPDATE messages
       SET payload = json_set(payload, '$.agentTrace', json(?1))
       WHERE conversation_id = ?2 AND message_id = ?3 AND json_valid(payload) = 1
```

三层缓解已存在：**只在终态写**、**400ms 覆盖式合并**、**单线程 flush**（`sub_message.rs:103-208`）。
注释原文已点明动机：*"writing the whole lead row on every spawn step serializes 100 workers onto one SQLite row."*

---

## 2. 体量分布（谁占地方）

| 字段 | lead | scoped | 合计 | 占比 |
|---|---|---|---|---|
| `content` | 2,710,357 | 15,219,873 | 17,930,230 | **43.4%** |
| 其它（id/status/agentChain/traceId/spawnDepth/…） | 1,235,592 | 4,967,430 | 6,203,022 | 15.0% |
| `reasoning` | 3,690,904 | 5,258,277 | 8,949,181 | **21.7%** |
| `toolCalls[]` | 1,996,585 | 5,783,638 | 7,780,223 | **18.8%** |
| `rawContent` | 402,905 | 0 | 402,905 | 1.0% |
| `attachments` | 2,233 | 0 | 2,233 | ~0 |
| `uiBindings` | 2,162 | 0 | 2,162 | ~0 |
| **`agentTrace[]`** | **6,819** | 0 | **6,819** | **0.017%** |
| `imagesBase64` | 0 | 0 | 0 | 0（未落库） |
| `toolRawOutput` | 0 | 0 | 0 | 0（未落库） |
| **合计** | **10,047,557** | **31,229,218** | **41,276,775** | 100% |

**字段出现行数**：`toolCalls` 2846 行 / `rawContent` 629 行 / `agentTrace` **6 行** / `attachments` 6 行 / `imagesBase64` 0 / `toolRawOutput` 0。

**单行体量**：平均 lead 3,816 / scoped 4,820 字符；最大 lead 212,666（工具结果行，`content` 181,711）/ scoped 95,188。

---

## 3. 写行为（谁被反复写）

| 字段 | 写后改 | 写方式 | 频率 | 单次代价 |
|---|---|---|---|---|
| **`agentTrace`** | ✅ | `json_set(payload,'$.agentTrace')` | 终态（100 路 → ~100 次/回合，400ms 合并） | **整行 10–33 KB** |
| **`toolCalls[]`** | ✅ | **整行 upsert** | 每次工具状态变化（本会话 5674 个工具） | 整行 3.8–4.8 KB |
| `content` / `rawContent` / `reasoning` | ✅ | 整行 upsert | 轮末 | 整行 |
| `contextState` | ✅ | `json_set` / `json_remove` | 上下文压缩 / 排除时 | 整行 + FTS |
| `is_milestone` / `is_system_generated` / `context_included` | ✅ | 列级 `UPDATE` | 少量 | 整行 + **FTS 重写** |
| `attachments` | ✅ | 整行 upsert | 媒体落地时 | 整行 |

---

## 4. 实测放大倍数（6 个锚行）

| message_id | 整行 payload | `agentTrace` | trace 数 | 倍数 | 其中 `reasoning` | 其中 `toolCalls` | 其中 `content` |
|---|---|---|---|---|---|---|---|
| `msg_950b7860…` | 33,132 | 1,702 | 3 | **19.5×** | 22,782 | 7,748 | 75 |
| `msg_00e8cbbe…` | 22,653 | 1,707 | 3 | 13.3× | 13,906 | 6,297 | 75 |
| `msg_880037cb…` | 17,206 | 1,135 | 2 | 15.2× | 10,924 | 4,315 | 134 |
| `msg_e957c61b…` | 17,179 | 1,137 | 2 | 15.1× | 9,757 | 5,626 | 80 |
| `msg_82339934…` | 10,226 | 568 | 1 | 18.0× | 6,195 | 2,888 | 45 |
| `msg_8476ddad…` | 12,888 | 570 | 1 | 22.6× | 6,786 | 4,892 | 49 |
| **合计** | **113,284** | **6,819** | 12 | **16.6×** | 70,350 | 31,766 | 458 |

> **写 1 字节 `agentTrace`，实际重写 ≈ 17 字节 payload**；而重写内容里 62% 是**与 trace 无关的 `reasoning`**。
> 也就是说：`agentTrace` 是"搭便车"付出了整行的代价。

---

## 5. 放大机制（三条）

| # | 机制 | 说明 |
|---|---|---|
| 1 | **整行重写** | `payload` 是单个 TEXT 列；`json_set` 需解析整个 JSON 再写回整行。SQLite 层面没有"只改一个字段"的能力 |
| 2 | **FTS 连带重写** | `messages_au AFTER UPDATE` 存在 → 每次 patch 都重写该行 FTS 条目（含 `content` 全文） |
| 3 | **数组全量替换 + 内存深拷贝** | 每次 flush 传整个 `agentTrace` 数组（非增量）；`sync_anchor_agent_trace_index` 每个终态事件都 `to_vec()` 整个数组 |

补注：机制 3 的 O(T²) 在当前 T=1–3 时无感；**真正吃代价的是机制 1、2**（与 T 无关，恒定 17×）。

---

## 6. 可选修法（按收益 / 成本）

| 优先级 | 动作 | 收益 | 成本 |
|---|---|---|---|
| 1 | `agentTrace` 搬出 payload：独立表 `agent_traces(conversation_id, anchor_message_id, trace_id, json, updated_at)`，按 `trace_id` upsert | 单次写 33 KB → ~1.7 KB（**≈19×**）；**独立表无 FTS 触发器** → 机制 2 自动消失 | 中（后端 + 迁移） |
| 2 | "写后改"字段统一走独立表/列，不再整行重写 payload | 覆盖所有同类字段（`toolCalls`、`contextState`、里程碑标记…） | 大（存储结构） |
| 3 | flush 只写本次变更的 trace id + 内存按 id 局部更新（去掉 `to_vec()`） | 消掉机制 3 | 小 |
| 4 | `reasoning` 单独评估（21.7% 体量、写后不变，却每次被搭便车重写） | 降低整行重写主体 | 小-中 |

**现成先例（可直接照抄）**

- `toolRawOutput`：注释写明 *"stripped from conversation-store payloads (large dumps historically made `load_conversation_messages` very slow)"* → 用**不落库**解决。
- 工具结果 body：前端 **pin / release** 模型（`ensureToolCallBody` / `releaseToolCallBody`）→ 大内容按需加载、用完释放。

---

## 7. 未验证项（如实标注）

1. **FTS 索引体积无法从 CLI 量**：`messages_fts` 使用 `cjk_bigram` 分词器（由应用注册），CLI 查询报 `no such tokenizer: cjk_bigram`。机制 2 的代价目前**只有代码证据**（触发器存在），没有实测数字。
2. **每回合实际 flush 次数**未埋点实测（机制推导：终态分散时 ~T 次，集中时 1 次）。
3. **流式 `content` 的实际写入频率**未实测（已看到的 upsert 点都在轮末/工具终态，非每 chunk；前端 `persistAppend` 的节流策略未查）。

---

## 8. 规模外推（100 路子 agent 的语境）

- 体量：`agentTrace` 仍是 0.02% 级 → 不是体量瓶颈；
- 写次数：100 个终态 → 最多 100 次 flush（合并后可能更少）→ 每次整行 10–33 KB + 一次 FTS 重写；
- 真正的风险排序（去掉人为上限与 SSE 后）：**① 前端每事件 `findMessage` O(C+M) + 单个大 computed；② 本文件的"整行重写 + FTS"；③ 渲染 DOM；④ 归属树的 memo 粒度**。

---

## 9. 与"子 agent 归属渲染"的关系

| | 归属渲染问题 | payload 写放大问题 |
|---|---|---|
| 层 | 前端渲染（`AssistantModelMessage` 的消息局部配对） | 后端持久化（`messages.payload` 单列 JSON） |
| 根因 | 把"数组边界"当成归属边界 | 把高频小字段塞进低频大 payload |
| 交叉点 | `agentTrace`（前端读它做归属；后端写它在 payload 里） | 同上 |
| 可否独立修 | ✅ 可（P0/P1 纯前端，零后端改动） | ✅ 可（后端搬表，前端不受影响） |

**共同模式**：*把生命周期不同的东西放在同一个容器里*（归属 ≠ 位置；高频字段 ≠ 低频大字段）。

---

## 10. 改造方案：agent 存储双层化（提案，2026-10-04）

> 来源：用户提案（2026-10-04）。本节只记录**方案与评估**，不代表已排期。
> 定位：本节是 §6「可选修法」的**根本方案**；§6 的「把 `agentTrace` 搬出 payload」只是本方案的一个子集。

### 10.1 提案原文

1. **统一逻辑 + 显式绑定**：所有操作（顶层 agent 或子 agent）逻辑上完全一致；父 agent 与子 agent 之间通过 **`toolCallId + messageId`** 关联绑定；界面以此为基准显示。
2. **双层 map 存储**：内部存储结构上，所有会话与子 agent 是 **`session_id × instanceId`** 的双层 map，每个 agent 占一个 value 对象；界面/内存中的更新彼此独立、互不干扰。

### 10.2 术语映射（提案 → 现状代码）

| 提案 | 现状对应物 |
|---|---|
| `session_id` | `conversation_id` |
| `instanceId` | `agentInstanceId` / `SpawnId` |
| 「每个 agent 一个 value 对象」 | 前端 `ScopedInstanceTranscript`（**已存在**） |
| `toolCallId + messageId` 绑定 | `AgentTrace.parentToolCallId` + `AgentTrace.anchorMessageId`（**已存在**） |

### 10.3 提案 1 评估：**已是现状的终点，等于 P1**

| 提案要求 | 现状 |
|---|---|
| 顶层/子 agent **逻辑完全一致** | ✅ **后端已是**：`run_sub_agent` 与 lead 共用 provider / stream / tool pass 循环（`sub_agent.rs`、`agent_stream_round.rs`、`agent_tool_pass/`），子 agent 只是「深度 +1、预算受限」的同一循环 |
| 父子通过 **toolCallId + messageId** 绑定 | ✅ **字段已是**：`parentToolCallId`（派发行）+ `anchorMessageId`（该层 scoped 行锚点） |
| 界面**以此为基准**显示 | ❌ **唯一没做到的一条**：渲染层仍按「消息局部配对」（`AssistantModelMessage.vue:60-83`） |

结论：提案 1 的实质改动只有一处 —— **渲染层改用这两个字段解析归属**，即 **P1**。它不是替代 P0/P1，而是 P0/P1 的目标状态描述。

**要精确的表述**：统一的是**机制**，不同的是**参数**；以下 4 处差异是刻意的，不应被「统一」掉：

| 维度 | lead | 子 agent |
|---|---|---|
| 上下文注入 | 系统提示 / 技能 / 任务板全量 | 子集（`SubAgentLoopContext`） |
| 预算 | 会话级 | `SessionToolBudget` + `max_spawn_depth` + `maxChildrenPerAgent` |
| UI 位置 | 顶格 | 挂在派发行下 |
| 审批 | 宿主行 | `pending_approval` 的宿主行 |

### 10.4 提案 2 评估：**前端已有雏形，后端是主战场**

| | 现状 | 提案 2 |
|---|---|---|
| **前端** | ✅ **已是双层 map**：`states: Map<convId, ConvState>`，内层 `bySpawn: Map<SpawnId, transcript>` + `rowById` + `instancesByAnchor`（`conversationScoped/store.ts:11-16`） | 已具备 |
| **后端** | ❌ scoped 行**平铺在 `messages` 表**（`is_scoped=1`，本会话 6479 行），靠 `payload.agentTrace` 数组 + 400ms `json_set` 补丁维护索引 | 需新建「实例 → 对象」存储 |

**「修补感」的来源**：lead 与子 agent 现在是**两套存储形态** —— lead 是「表里的行」，子 agent 是「别人 payload 里的数组 + 平铺的行」。提案 2 正是把两套合成一套。

#### 收益（可量化）

| 指标 | 现状 | 提案 2 之后 |
|---|---|---|
| 写放大（`agentTrace`） | **17×**（写 1.7 KB → 重写 33 KB 整行 + FTS） | **1×**（只写该实例 value） |
| 事件落地 `findMessage` | **O(C + M)**，M=2633，每事件一次线性扫 | **O(1)**（两次哈希：conv → instance → row） |
| 失效粒度 | 一个大 computed + 一个 payload 行 → 全量重算 | 单实例 value → 只有它自己重算 |
| 挂载归属 | 渲染层「猜」（消息局部配对） | **本地查询**（归属即数据结构） |

#### 代价 / 风险

| # | 风险 | 说明 |
|---|---|---|
| **1** | **UI 时间线从哪来** ⚠️ | 现顺序来自 `messages.position`（会话内全局单调）。改成「每实例一个对象」后，「lead 说话 → 派发 → 子 agent 输出 → lead 继续」的顺序需要**新的合成规则**。**必须先回答**，否则会变成「每个 agent 都对，但拼不成一条时间线」 |
| 2 | 迁移 | 4.8 GB 存量 + 已持久化的 payload 结构；需定「一次性迁移」还是「新表双写 + 读时兼容」 |
| 3 | 事务边界 | 现在是单行 `UPDATE`；之后是多表/多对象写入 → 需要事务或批处理 |
| 4 | 搜索 | FTS 现按 `messages` 行建；新结构要重建索引面（scoped 行搜索现用 `data-sub-agent-content-id` 定位） |
| 5 | 裁剪 / 压缩 | 现按 messages 行 trim；新结构要定义「哪些实例可删、删后 anchor 怎么显示」 |

#### 它解决不了的两件事（不要期待过高）

| 瓶颈 | 提案 2 是否解决 |
|---|---|
| 渲染 DOM（100 个完整帧同时挂载） | ❌ 仍需 stub + 虚拟滚动 |
| 单条 SSE 通道的事件量 | ❌ 仍需分层订阅 / 节流（事件已按实例寻址，但通道仍是一条） |

### 10.5 必须先回答的问题：UI 时间线从哪来

| 选项 | 做法 | 代价 |
|---|---|---|
| **① 保留全局单调 `position`（推荐）** | 所有 agent 的行仍共享一个会话内单调序号；双层 map 只改「归属与写入」，不改「排序依据」 | 改动最小；排序仍是现成机制 |
| ② 合成顺序 | 按「回合 → 派发点 → 子流」在渲染期合成顺序 | 更彻底，但要重写排序与虚拟滚动 |

### 10.6 与 P0–P4 的关系：**不是替代，是前置**

双层 map 之后，渲染层**依然需要**「按 `(anchorMessageId, parentToolCallId)` 组装」这套逻辑。

```
P0 ✅ 回合级归属选择器（归属按数据算）
  → P1  渲染按数据挂载              ← 提案 1 落地
  → P2/P3 折叠投影化 + 嵌套挂载      ← 提案 2 的渲染侧基础
  → P4  释放策略独立
  → 【提案 2】存储双层化（后端为主）  ← 从根上消除「两套形态」
```

**P0/P1 不是修补，是提案 2 的必经前置。**

### 10.7 分期草案（待定）

| 阶段 | 内容 | 前置 |
|---|---|---|
| S0 | 定稿顺序规则 + 数据结构（表 / 索引 / value 对象字段） | §10.5 |
| S1 | 后端新表 + **双写**（老 payload 保留，读仍走老路径） | S0 |
| S2 | 读路径切新表（payload 作为兼容回退） | S1 |
| S3 | 停写 `payload.agentTrace`；清理旧字段写入 | S2 |
| S4 | 存量迁移（可选，按需） | S3 |

### 10.8 未决问题清单

1. **UI 时间线**（§10.5）—— 唯一阻塞 S0 的问题；
2. 迁移策略：一次性迁移 vs 双写；
3. 事务边界（多对象写入）；
4. 搜索索引面（FTS 重建方式）；
5. 裁剪 / 压缩语义（哪些实例可删）；
6. 提案 2 与前端 `conversationScoped` 的合并方式（复用还是重写）。
