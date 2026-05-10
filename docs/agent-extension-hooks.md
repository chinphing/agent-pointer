# Agent 扩展钩子（Extension Hooks）

本文档说明 **pointer-app** 中与 Python 项目 **Pointer**（`PyProjects/pointer`）里 `python.helpers.extension` 相对应的插件机制：扩展点在何时触发、如何注册、如何与 Computer 等 Agent 协作。

实现位置：`crates/pointer-core/src/extensions/`。

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
| `messages` | 本次即将发给模型的 **HTTP 消息列表的可变借用**。起始内容为某条「基础历史」的克隆（见第 4、5 节）；钩子通常**追加** ephemeral 的 `User` 消息（如带 `images_base64`），**不会**写回会话持久化的 `history`。 |
| `conversation_id` | 当前会话 id（与前端/Tauri 流一致）。 |
| `stream` | 可选的 `ChatStreamSender`；若存在，钩子可发送 **`StreamEvent::UiToast`**（仅界面横幅提醒，**不**写入聊天记录、**不**进入模型 payload）。 |

**`BeforeMainLlmCallContext`**

| 字段 | 含义 |
|------|------|
| `computer_state` | 同上。 |
| `lead_agent_profile` | 同上，与本轮 `stream_chat` 的「说话者」一致。 |

---

## 4. 注入节点与时间线（单轮模型请求）

下面按**时间先后**描述一次「模型被调用」之前发生了什么。代码路径：`chat_service.rs` 中单智能体主循环 `run_chat_inner` 内的 `loop`，以及 `run_sub_agent` 内的子循环（两者**同构**，区别只在 `messages` 的初始快照，见第 5 节）。

### 4.1 时间轴（粗粒度）

在同一轮迭代里，顺序固定为：

1. **进入本轮** — 检查取消、工具预算；生成本轮 `assistant_id`（UI 流式用）；可选 Supervisor 占位 trace。
2. **准备 Provider** — 新建 `OpenAIProvider`、channel；尚未发 HTTP。
3. **构造 API 消息列表** — `messages = <基础历史>.clone()`（单智能体：`history`；子 Agent：`local_history`）。
4. **`message_loop_prompts_after`** — `run_message_loop_prompts_after`：可修改 `messages`（例如追加屏幕注入）。
5. **`before_main_llm_call`** — `run_before_main_llm_call`：只读上下文为主，默认可为空操作。
6. **组装 system 侧 prompts** — `build_env_context`、session inject、agent system prompts、工具 markdown、`xml_tool_prompt` 等拼成 `prompts_clone`（与 Python「system + extras」一侧对应，Rust 里作为单独参数传入 `stream_chat`）。
7. **`stream_chat`** — `tokio::spawn` 里带着 **`&history_for_api`（即上面的 `messages`）** 和 **`prompts_clone`** 请求模型；之后才是流式 delta、工具解析、写回持久化 `history` 等。

要点：**扩展钩子在第 4～5 步执行，严格发生在「system 文本拼完」之前还是之后？**  
在当前实现里，**system 拼接在第 6 步**，钩子 **在第 4～5 步**，因此钩子执行时 **还看不到** 最终的 `prompts_clone` 全文；钩子只能依赖 `Context` 里已有字段和 `messages`。若某钩子需要「完整 system」，需要把拼接提前或向 `Context` 传入预览字符串（当前未做）。

与 Python 的细微差别：Python 在 `prepare_prompt` 里先写入 `loop_data.system` / `history_output` 再跑 `message_loop_prompts_after`，扩展**可以**读到已组好的 system 片段；Rust 当前是 **先改 user 侧 `messages`，再组 system**，若要对齐「先 system 后扩展」，需重构 `chat_service` 顺序。

### 4.2 单智能体：`messages` 在注入时刻包含什么

- **来源**：`history.clone()`，即当前会话中**已持久化**的多轮消息（user / assistant / 工具轮次 flatten 前的结构由后续 `make_openai_messages` 处理）。
- **注入追加**：例如 Computer 在列表**末尾**追加一条仅用于本次请求的 `User` 消息（带 `[CUR_SCREEN]` 文本与可选 `images_base64`）。
- **不落盘**：本轮结束后，持久化 `history` 仍按原逻辑只追加**真实的** assistant / tool 消息；**不会**把这条 ephemeral 注入写进会话存储。

### 4.3 一轮内的多次模型调用（工具循环）

用户发一条消息后，可能经历多轮「模型 → 工具 → 再模型」。**每一轮**新的模型请求都会重复上述 3～7 步：

- 每一轮都会重新 `history.clone()`（此时 `history` 已包含上一轮 assistant 与 tool 结果）。
- 每一轮都会再次执行 `message_loop_prompts_after` / `before_main_llm_call`。  
因此 Computer **每一轮都会重新截图+标注**（与 Python 每轮 inject 一致）。

### 4.4 序列图（与 4.1 一致）

```mermaid
sequenceDiagram
    participant Loop as 消息循环（每轮工具周期）
    participant Hist as 持久化 history
    participant Msg as messages（API 快照）
    participant Ext1 as message_loop_prompts_after
    participant Ext2 as before_main_llm_call
    participant Sys as 拼接 system prompts
    participant LLM as stream_chat

    Loop->>Hist: 读取当前 history（上一轮已 push）
    Loop->>Msg: messages = history.clone() 或 local_history.clone()
    Loop->>Ext1: run_message_loop_prompts_after(ctx)
    Note over Ext1,Msg: 可追加 ephemeral User / 多模态
    Loop->>Ext2: run_before_main_llm_call(ctx)
    Loop->>Sys: prompts_clone = env + system + tools + xml…
    Loop->>LLM: stream_chat(messages, prompts_clone)
```

---

## 5. 子 Agent（Supervisor）：上下文如何传入、是否独立

Supervisor 模式下，规划器根据**主会话** `history` 生成多个 `AgentTask`；每个任务调用 `run_sub_agent`。

### 5.1 子 Agent 的「对话上下文」——**独立 mini 会话**

- 子 Agent **不使用**主会话的 `history` 作为模型输入。
- 初始 `local_history` **只有一条** `User` 消息：`content = task.instruction`（Supervisor 下发的子任务全文）。
- 若任务带 `dependsOn`，实现上会把依赖任务的输出摘要**前缀**拼进 `instruction`（`[Prior task outputs]` / `[Current task]`），仍是一条 user 消息，**不是**完整主聊天 transcript。
- 子 Agent 自己的多轮工具循环里，只在 `local_history` 上累加本轮 assistant、tool 等，与主 `history` **隔离**。

系统 prompt 侧子 Agent 另有：`env_context`、可选 `rendered_session_inject`、一段 **sub_agent_header**（明确说明「下一条 user 来自 Supervisor，**不包含主聊天历史**」）、skills、allowed tools、xml tool prompt。

**结论（对话语义）**：子 Agent 在**消息列表意义上是独立的**；它只「看见」任务描述 +（可选）前置任务摘要 + 自己多轮工具产生的历史。

### 5.2 子 Agent 与扩展钩子

- 每一轮子 Agent 的每次模型请求前，同样执行：
  - `messages = local_history.clone()`
  - `run_message_loop_prompts_after`（`lead_agent_profile = def.profile`，例如子 Agent 为 `computer` 时仍会注入屏幕）
  - `run_before_main_llm_call`
- 使用的 **`ExtensionRegistry` 与单智能体相同**（`AppState.extensions`），**不是**每子 Agent 一份。

### 5.3 与主会话「不独立」的共享资源（重要）

以下在进程内**全局共享**，主 Agent 与子 Agent **串行或交错使用**时会影响彼此：

| 资源 | 说明 |
|------|------|
| `AppState.computer_state` | 同一块 `VisionState`、同一套标注客户端与执行器。子 Agent `Computer` 若跑屏幕注入，会更新**同一** `index_map` / `screen_bbox`。主会话若也使用 Computer，或连续多个 Computer 子任务，后一轮会看到上一轮写入的视觉状态，除非在业务层清空或隔离。 |
| `AppState.tools` / `skills` / `agents` | 全局注册表，仅配置只读。 |
| 工具预算 `SessionToolBudget` | Supervisor 与子 Agent **共用**同一预算计数（传入 `run_sub_agent` 的 `tool_budget`）。 |

**结论（运行时）**：子 Agent **对话上下文独立**，**Computer 等带副作用的全局状态不独立**；设计扩展或并行子任务时需考虑 `VisionState` 与预算的语义。

### 5.4 Supervisor 规划阶段与扩展的关系

- `plan_agent_tasks` 使用 `provider.chat_once(history, &[planning_prompt], …)`：**不经过**本文档中的 `message_loop_prompts_after` / `before_main_llm_call`。
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

`before_main_llm_call` 当前**无**默认实现；与 Python `AttachSnapshotToAgentLog` 等对位的能力可在此扩展点追加。

---

## 8. 注册方式

### 8.1 默认（应用启动）

`AppState::new` 中大致顺序为：

1. `ExtensionRegistry::new()`
2. `extensions::register_builtin_extensions(&mut registry)`
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
- **上下文字段**：`BeforeMainLlmCallContext` 较精简；日志快照、conversation id 等可按需增量添加。
- **与 Python 顺序对齐**：可选重构为「先组装 system，再跑 `message_loop_prompts_after`」，以便钩子读取完整 system 文本。

---

## 11. 相关文档与代码

- 实现计划中的 Computer 数据流：`docs/computer-use-implementation-plan.md`
- 注册表与 trait：`crates/pointer-core/src/extensions/mod.rs`
- Computer 屏幕注入：`crates/pointer-core/src/agents/computer/extension_hooks/screen_inject.rs`（由 `extension_hooks/mod.rs` 汇总注册）
- 调用点：`crates/pointer-core/src/chat_service.rs`（搜索 `run_message_loop_prompts_after`、`run_before_main_llm_call`、`run_sub_agent`）
