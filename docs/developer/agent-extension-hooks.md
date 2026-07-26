# Agent 扩展钩子（Extension Hooks）

本文档说明 **pointer-app** 中与 Python 项目 **Pointer**（`PyProjects/pointer`）里 `python.helpers.extension` 相对应的插件机制：扩展点在何时触发、如何注册、如何与 Computer 等 Agent 协作。

实现位置：`crates/pointer-core/src/extensions/`。主对话里 **system 拼接块、env、task board、历史消息** 在 HTTP 中的先后关系（含「改前」基线说明）见 **[`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)**。

---

## 1. 设计目标

- **对齐 Python 的「扩展点」概念**：在消息循环的固定阶段插入异步逻辑，而不把分支写死在 `chat_service` 里。
- **显式注册**：Rust 侧**不**像 Python 那样按目录扫描 `extensions/<point>/`；钩子通过 `ExtensionRegistry` 的 `register_*` 方法注册（内置钩子在 `AppState::new` 里通过 `register_builtin_extensions` 完成）。
- **可替换**：同一 `override_key` 再次注册会**覆盖**旧钩子，便于定制或 A/B 实现，无需调整注册顺序。

---

## 2. 与 Python Pointer 的对应关系

| Python | Rust（pointer-core） |
|--------|----------------------|
| `Extension` 基类 + `async def execute(**kwargs)` | `async_trait`：`MessageLoopPromptsAfterHook` / `BeforeMainLlmCallHook` |
| `call_extensions("message_loop_prompts_after", …)` | `ExtensionRegistry::run_message_loop_prompts_after` |
| `call_extensions("before_main_llm_call", …)` | `ExtensionRegistry::run_before_main_llm_call` |
| 多路径合并后按**模块文件名**去重，**先出现的保留** | 按 `override_key` 去重，**后注册覆盖先注册**（见下文差异） |
| 去重后按**文件名**字典序排序执行 | 按 `sort_key()` **字符串字典序**排序执行 |
| `agents/computer/extensions/...` 下放 `.py` | Computer 专用钩子放在 `agents/computer/extension_hooks/`；通用注册表在 `extensions/` |

**去重语义差异（重要）**

- **Python**：遍历路径时，同一「逻辑文件名」**第一次**出现的实现生效（高优先级路径覆盖低优先级）。
- **Rust**：同一 `override_key` **最后一次** `register_*` 生效，更符合「后装上的覆盖默认」的显式 API 习惯。若需模拟 Python「只读内置、不被覆盖」，不要对同一 `override_key` 再次注册。

---

## 3. 核心类型

### 3.1 `ExtensionRegistry`

- 挂在 **`AppState`** 上：`AppState.extensions: Arc<ExtensionRegistry>`。
- 内部维护两个列表（概念上对应两个扩展点）：
  - `message_loop_prompts_after`
  - `before_main_llm_call`
- 运行时对列表快照按 `sort_key` 排序后依次 `execute`；错误通过 `anyhow::Result` 向上传播，可中断本轮请求。

### 3.2 `override_key` 与 `sort_key`

每个钩子需实现：

| 方法 | 作用 |
|------|------|
| `override_key()` | 稳定标识，等价于 Python 侧用于去重的**源文件名**（如 `_10_computer_screen_inject`）。同一扩展点内相同 key 只保留**最后一次注册**。 |
| `sort_key()` | **执行顺序**，建议使用与 Python 一致的前缀风格：`_10_…`、`_20_…`、`_75_…`，按字符串比较排序。 |

两者可以相同（当前 Computer 屏幕注入即如此）。

### 3.3 上下文对象（字段级说明）

**`MessageLoopPromptsAfterContext`**

| 字段 | 含义 |
|------|------|
| `computer_state` | 全局唯一的 Computer 运行时状态：截图/标注客户端、共享的 `VisionState`（索引 → 像素）、动作执行器等。 |
| `lead_agent_profile` | **当前这一轮**要对话的 Agent 的 profile。单智能体模式下为主 lead 的 profile；Supervisor 子任务模式下为**该子 Agent 定义**的 profile（例如 `Computer` / `Coder`）。钩子用它决定是否为 no-op（如仅 `Computer` 才注入屏幕）。 |
| `base_messages` | **只读**：本轮权威 transcript（主会话 `history` 或子 Agent `local_history`）。钩子**不得**修改或向其 `push`。 |
| `injected_tail` | **可变**：本轮仅用于 API 的 ephemeral 行。钩子在此**追加**（如 `[CUR_SCREEN]` / task-board user 块）；**不会**写回持久化 `history`。 |
| `conversation_id` | 当前会话 id（与前端/Tauri 流一致）。 |
| `stream` | 可选的 `ChatStreamSender`；若存在，钩子可发送 **`StreamEvent::UiToast`**（仅界面横幅提醒，**不**写入聊天记录、**不**进入模型 payload）。 |
| `round_assistant_message_id` | 可选；本轮助手消息 id（与主循环 `MessageStart` 一致，或 Supervisor 子任务下**父级**助手气泡 id）。注入用它发送 **`StreamEvent::AssistantRoundScreen`**。 |
| `round_screen_dump_prefix` | 可选；落盘调试图时的文件名前缀，缺省同 `round_assistant_message_id`。子 Agent 每轮迭代用自己的 id，避免与父消息 id 混用。 |

**`StreamEvent::AssistantRoundScreen`** 只携带 `annotatedRelPath`（相对于与设置/技能相同的应用数据根目录下的 `PointerApp/computer-captures/`；**仅 `debugMenusEnabled` 调试模式**时落盘），避免把大图 base64 塞进流与内存；UI 在点击预览时读盘：**Tauri** 用 `preview_computer_round_screen`，**pointer-server** 用 `GET /api/computer/round-screen-preview?relPath=…`（与 `GET /api/computer/annotated-preview` 对应 Tauri 的 `preview_computer_annotated_screen`）。**桌面端与 server 端启动时**都会执行相同的 **7 天**截图目录清理（`capture_debug::CAPTURE_RETENTION_DAYS`），若有删除则向事件总线发送 **`UiToast`**（`conversationId` 为空 = 全局「截图过期已清理」）。

**`BeforeMainLlmCallContext`**

| 字段 | 含义 |
|------|------|
| `computer_state` | 同上。 |
| `lead_agent_profile` | 同上，与本轮 `stream_chat` 的「说话者」一致。 |
| `system_prompts_dynamic` | **可变**：本轮 **dynamic** 分区（常见为 `[LOCKED GOAL]` 等）。**cacheable** 已在钩子前含公共通信、Agent/Skills、工具附录、**`[Environment]`**。详见 **[`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)**。 |
| `conversation_id` | 主会话 id（流式/UI）；子 Agent 下仍为**父会话** id。 |
| `task_board_store` | `Arc<TaskBoardStore>`，供内置或自定义钩子读取任务板。 |
| `task_board_store_key` | 传入 `TaskBoardStore::snapshot_for_prompt` 的键：主会话为 `conversation_id`；Supervisor 子 Agent 为 `sub_agent_task_board_store_key(...)` 的复合键。 |

---

## 4. 注入节点与时间线（单轮模型请求）

下面按**时间先后**描述一次「模型被调用」之前发生了什么。代码路径：`single_agent.rs` 中 `run_single_agent_loop`；每轮 prompt 组装在 `single_agent_prompt.rs`；流式收包在 `single_agent_stream.rs`（内部共用 `agent_stream_round.rs`）；流后决策（空工具 / envelope 重试）在 `agent_post_stream.rs`（Lead 经 `single_agent_post_stream.rs` 薄封装）；工具落地在 `agent_tool_pass.rs`（Lead 经 `single_agent_tools.rs` 薄封装）。子循环见 `sub_agent.rs` + `sub_agent_prompt.rs` + `sub_agent_stream.rs`，与 Lead **同构**并共用上述 shared 模块；区别在 `messages` 初始快照与格式重试投递方式，见第 5 节。

### 4.1 时间轴（粗粒度）

在同一轮迭代里，顺序固定为：

1. **进入本轮** — 检查取消、工具预算；生成本轮 `assistant_id`（UI 流式用）。
2. **`MessageStart`（及 Supervisor 的 `AgentStep`）** — 先创建前端助手气泡，再跑注入，以便把本圈截图事件绑定到该 `message_id`。
3. **准备 Provider** — 新建 `OpenAIProvider`、channel；尚未发 HTTP。
4. **准备 API 输入** — 只读借用基础历史（单智能体：`history`；子 Agent：`local_history`），新建空的 `injected_tail`，并填入 `round_assistant_message_id`。
5. **`message_loop_prompts_after`** — `run_message_loop_prompts_after`：仅向 `injected_tail` 追加 ephemeral 行（例如屏幕注入）。
6. **组装 system（cacheable）** — 公共通信、Agent/Skills、工具附录，再 `push_env_to_cacheable`（`[Environment]`）。
7. **`before_main_llm_call`** — 钩子向 **`system_prompts_dynamic`** 追加（例如 `[LOCKED GOAL]`）。
8. **组 wire → `stream_chat_wired`** — `build_stream_chat_wire(base, injected_tail, SystemPromptSections)` → `make_openai_messages_with_inject`（千问见 **[`qwen-context-cache.md`](../llm/qwen-context-cache.md)**）；HTTP 任务只持有 wire JSON，不再持有完整 `history` 克隆。Computer **完整墙钟时间**在 **`[CUR_SCREEN]`** `user` 消息中（`screen_inject.rs`）。

要点：task board 已迁移为 `message_loop_prompts_after` 的末尾 user 注入块，不再占用 system cacheable/dynamic。长会话峰值内存见 **[`../internals/long-chat-memory.md`](../internals/long-chat-memory.md)**。

### 4.2 单智能体：注入时刻的消息视图

- **基础历史**：会话中已持久化的多轮消息（只读借用；user / assistant / 工具轮次 flatten 前的结构由后续 `make_openai_messages_with_inject` 处理）。
- **注入追加**：例如 Computer 向 `injected_tail` **末尾**追加一条仅用于本次请求的 `User` 消息（**`Local wall-clock at capture:`** 含完整日期时间，接 **`[CUR_SCREEN]`** 与可选 `images_base64`）。
- **不落盘**：本轮结束后，持久化 `history` 仍按原逻辑只追加**真实的** assistant / tool 消息；**不会**把 ephemeral 注入写进会话存储。

### 4.3 一轮内的多次模型调用（工具循环）

用户发一条消息后，可能经历多轮「模型 → 工具 → 再模型」。**每一轮**新的模型请求都会重复上述 4～8 步：

- 每一轮都只读借用当前 `history`（已包含上一轮 assistant 与 tool 结果），并新建本轮 `injected_tail`。
- 每一轮都会再次执行 `message_loop_prompts_after` / `before_main_llm_call`（task board 摘要在 user 末尾块刷新，有内容时）。  
因此 Computer **每一轮都会重新截图+标注**（与 Python 每轮 inject 一致）。

### 4.4 序列图（与 4.1 一致）

```mermaid
sequenceDiagram
    participant Loop as 消息循环（每轮工具周期）
    participant Hist as 持久化 history
    participant Tail as injected_tail
    participant Ext1 as message_loop_prompts_after
    participant Sys as 拼接 system prompts
    participant Ext2 as before_main_llm_call
    participant Wire as build_stream_chat_wire
    participant LLM as stream_chat_wired

    Loop->>Hist: 只读借用当前 history
    Loop->>Tail: 新建空 injected_tail
    Loop->>Ext1: run_message_loop_prompts_after(ctx)
    Note over Ext1,Tail: 向 tail 追加 [CUR_SCREEN] 等
    Loop->>Sys: cacheable = 公共 / Agent / 工具 / Environment
    Loop->>Ext2: run_before_main_llm_call(ctx)
    Note over Ext2,Sys: dynamic += [LOCKED GOAL] 等
    Loop->>Wire: base + injected_tail + SystemPromptSections
    Loop->>LLM: 仅持有 wire JSON 发 HTTP
```

---

## 5. 子 Agent（Supervisor）：上下文如何传入、是否独立

Supervisor 模式下，规划器根据**主会话** `history` 生成多个 `AgentTask`；每个任务调用 `run_sub_agent`。

### 5.1 子 Agent 的「对话上下文」——**独立 mini 会话**

- 子 Agent **不使用**主会话的 `history` 作为模型输入。
- 初始 `local_history` **只有一条**短 stub `User` 消息；**`goal`** / **`context`** 由宿主写入 system dynamic（**Assigned task**）。
- 若任务带 `dependsOn`，实现上会把依赖任务的输出摘要写入 **`context`**（`[Prior task outputs]`），**不**拼进 `goal`。
- 子 Agent 自己的多轮工具循环里，只在 `local_history` 上累加本轮 assistant、tool 等，与主 `history` **隔离**。

系统 prompt 侧子 Agent 与主轮同构：**cacheable** 由共享函数 `push_agent_role_cacheable_prompts` 组装（`COMMUNICATION_PUBLIC` + Computer **tier** 切片，或非 Computer 的 `system_prompt`），再追加 **sub_agent_header**（Computer 仅短交接说明，不含烘焙 `agent.system_prompt()`）、skills、工具附录、Environment；task board 由末尾 user 注入提供（有内容时）。

**结论（对话语义）**：子 Agent 在**消息列表意义上是独立的**；它只「看见」任务描述 +（可选）前置任务摘要 + 自己多轮工具产生的历史。

### 5.2 子 Agent 与扩展钩子

- 会话初始化与每轮 prompt 组装在 **`sub_agent_prompt.rs`**（`init_sub_agent_session` / `prepare_sub_agent_round_prompts`）；流式收包在 **`sub_agent_stream.rs`**（内部共用 **`agent_stream_round.rs`**）；流后决策与工具执行分别共用 **`agent_post_stream.rs`** / **`agent_tool_pass.rs`**（与 Lead 同构，见 §5.2.1）。
- 每一轮子 Agent 的每次模型请求前，同样执行：
  - 只读借用 `local_history`，新建 `injected_tail`
  - `run_message_loop_prompts_after`（`lead_agent_profile = def.profile`，例如子 Agent 为 `computer` 时仍会注入屏幕）
  - **cacheable** = `push_agent_role_cacheable_prompts` + 子 Agent `session_extras` + **`tools_system_appendix`** + Environment
  - `run_before_main_llm_call`（dynamic 追加其他系统动态块；`task_board_store_key` 仍用于任务板读写/注入）
  - `build_stream_chat_wire(local_history, injected_tail, …)` 后 `stream_chat_wired`
- 使用的 **`ExtensionRegistry` 与单智能体相同**（`AppState.extensions`），**不是**每子 Agent 一份。

#### 5.2.1 Lead 与子 Agent 共用模块的差异（行为不变）

| 环节 | Lead（`single_agent.rs`） | Sub（`sub_agent.rs`） |
|------|---------------------------|------------------------|
| 流式 UI | `ContentDeltaMode::LeadMessage`（`RawContentDelta` + `Delta`） | `ContentDeltaMode::SubAgentTrace`（`emit_agent_content_delta`） |
| 格式重试 user 行 | `InjectedUserMessage` + 主 `history` | 仅 push 到 `local_history` |
| 工具预算耗尽文案 / 压缩 | `compress_for_session = true` | `compress_for_session = false` |
| 结束形态 | `Ok(())` | `AgentRunResult`（`response` 或自然语言无工具） |
| 工具 pass | 可 `run_subagent` 委派 | 硬拒绝 `run_subagent`，按 `allowed_tools` 校验 |

### 5.3 与主会话「不独立」的共享资源（重要）

以下在进程内**全局共享**，主 Agent 与子 Agent **串行或交错使用**时会影响彼此：

| 资源 | 说明 |
|------|------|
| `AppState.computer_state` | 同一块 `VisionState`、同一套标注客户端与执行器。子 Agent `Computer` 若跑屏幕注入，会更新**同一** `index_map` / `screen_bbox`。主会话若也使用 Computer，或连续多个 Computer 子任务，后一轮会看到上一轮写入的视觉状态，除非在业务层清空或隔离。 |
| `AppState.tools` / `skills` / `agents` | 全局注册表，仅配置只读。 |
| 工具预算 `SessionToolBudget` | **外层**编排（单智能体主循环或 Supervisor 每完成一个子任务）与 **内层**子 Agent 工具循环 **分开计数**：每次 `run_sub_agent` 使用 **新的**内层预算实例（上限来自 `maxSubAgentToolRounds`）；外层在包含工具执行的一轮结束时 `record_tool_cycle` 一次（含 `run_subagent` 所在轮）。 |

**结论（运行时）**：子 Agent **对话上下文独立**，**Computer 等带副作用的全局状态不独立**；设计扩展或并行子任务时需考虑 `VisionState` 与预算的语义。

### 5.4 Supervisor 规划阶段与扩展的关系

- `plan_agent_tasks`（`supervisor_plan.rs`）/ `synthesize_final_answer`（`supervisor_synth.rs`）使用 `provider.chat_once(history, &[planning_prompt], …)`：**不经过**本文档中的 `message_loop_prompts_after` / `before_main_llm_call`；编排入口为 `supervisor.rs` 的 `run_supervisor_chat`。
- 仅**子 Agent（及单智能体主循环）**在 `stream_chat` 前走扩展链。

---

## 6. 与 Python `prepare_prompt` 的对应关系（摘要）

- Python：在设置 `loop_data.system` 与 `loop_data.history_output` 之后调用 `message_loop_prompts_after`。
- Rust：先对 **API 用 `messages`** 跑扩展，再拼接 **system 侧 `prompts_clone`** 并 `stream_chat`。若要对齐 Python「扩展可读完整 loop_data.system」，需调整顺序或扩展 `Context`。

---

## 7. 内置钩子一览

| override_key / sort_key | 扩展点 | 文件 | 行为摘要 |
|-------------------------|--------|------|----------|
| `_10_computer_screen_inject` | `message_loop_prompts_after` | `agents/computer/extension_hooks/screen_inject.rs` | 当 `lead_agent_profile == Computer` 时：`capture_and_annotate`，向 `messages` 追加带 PNG base64 的临时 user 消息；失败则追加纯文本说明。 |
| `_99_common_user_dynamic_inject` | `message_loop_prompts_after` | `extensions/common_user_dynamic_inject_hook.rs` | 在本轮 `messages` 末尾追加 user 注入块：`task_board` Markdown（主会话或子任务键；有内容或 init hint 时）。 |

自定义钩子可 **替换** 同 `override_key` 的 `_99_common_user_dynamic_inject` 以改变格式或关闭注入。

---

## 8. 注册方式

### 8.1 默认（应用启动）

`AppState::new` 中大致顺序为：

1. `ExtensionRegistry::new()`
2. `extensions::register_builtin_extensions(&mut registry)`（包含 `_10_computer_screen_inject`、`_99_common_user_dynamic_inject` 等）
3. `Arc::new(registry)` 存入 `AppState.extensions`

### 8.2 增加或覆盖钩子

在获得可变 `ExtensionRegistry` 的前提下（例如自定义 `AppState` 构造）：

```rust
use std::sync::Arc;
use pointer_core::extensions::{ExtensionRegistry, MessageLoopPromptsAfterHook, /* … */};

let mut registry = ExtensionRegistry::new();
pointer_core::extensions::register_builtin_extensions(&mut registry);

// 覆盖 Computer 屏幕注入（相同 override_key）
registry.register_message_loop_prompts_after(Arc::new(MyComputerScreenHook));

let extensions = Arc::new(registry);
```

实现新钩子时需使用 `async_trait::async_trait`，并保证 `Send + Sync`（通常为无内部可变共享状态的 `struct` 或 `Arc` 内字段）。

### 8.3 辅助函数

- `extensions::new_extension_message_id(prefix)` — 生成临时消息 id（如 `screen_inject_*`）。
- `extensions::now_ms()` — 消息 `created_at` 时间戳，与聊天其它路径一致。

---

## 9. 实现新钩子的检查清单

1. 选定扩展点：`message_loop_prompts_after` 或 `before_main_llm_call`。
2. 为钩子选择**全局唯一**的 `override_key`（若不想替换内置 Computer 注入，勿使用 `_10_computer_screen_inject`）。
3. 设置 `sort_key`，确保与同一扩展点内其它钩子的相对顺序符合预期。
4. 在 `execute` 内根据 `AgentProfile` 等自行决定是否 no-op（与 Python 各扩展内判断 `profile` 一致）。
5. 对 `MessageLoopPromptsAfter`：**不要**假设 `messages` 会持久化到会话存储；仅影响本次 HTTP 请求载荷。
6. 需要更多上下文时，优先扩展 `*Context` 结构体并在 `chat_service` 中传入，避免在钩子里拉取全局单例。
7. 若钩子依赖 **Computer 全局状态**，需同时考虑 **子 Agent** 与 **主会话** 交错执行时的 `VisionState` 语义（见第 5.3 节）。

---

## 10. 限制与后续方向

- **无动态扫盘**：不支持运行时从 `usr/extensions` 加载 `.so` 或脚本；扩展均为编译进 `pointer-core` 或通过上层 crate 注册。
- **扩展点数量**：目前仅实现与 Computer 管线强相关的两个点；若要对齐 Python 的 `tool_execute_before`、`response_stream_chunk` 等，需新增 trait、`ExtensionRegistry` 字段及在 `provider` / 工具执行路径上显式 `run_*`。
- **上下文字段**：`BeforeMainLlmCallContext` 含可变的 **`system_prompts_dynamic`** 与 `task_board_store` / `task_board_store_key`；自定义钩子可替换同 `override_key` 的内置任务板快照行为。
- **与 Python 顺序对齐**：可选重构为「先组装 system，再跑 `message_loop_prompts_after`」，以便钩子读取完整 system 文本。

---

## 11. 相关文档与代码

- 实现计划中的 Computer 数据流：[`computer-use-implementation-plan.md`](../design/computer-use-implementation-plan.md)
- 注册表与 trait：`crates/pointer-core/src/extensions/mod.rs`
- Computer 屏幕注入：`crates/pointer-core/src/agents/computer/extension_hooks/screen_inject.rs`（由 `extension_hooks/mod.rs` 汇总注册）
- 调用点：`session_inner.rs`、`single_agent.rs`、`single_agent_prompt.rs`、`single_agent_stream.rs`、`agent_stream_round.rs`、`agent_post_stream.rs`、`agent_tool_pass.rs`、`sub_agent.rs`、`sub_agent_prompt.rs`、`sub_agent_stream.rs`（搜索 `run_message_loop_prompts_after`、`run_before_main_llm_call`、`run_sub_agent`）
