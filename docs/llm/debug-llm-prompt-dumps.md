# LLM 请求落盘调试

用于排查「每一轮实际发给模型的内容」时，可开启将请求体写入本地 JSON 文件。

## 开启方式

1. **设置**：在「界面配置」中打开「保存每轮对话请求」（对应 `ModelSettings.debugDumpLlmPrompts` / JSON 字段 `debugDumpLlmPrompts`）。
2. **环境变量**（不依赖设置项）：`POINTER_DEBUG_LLM_PROMPTS` 为 `1` 或 `true` 时强制开启。

## 控制台：打印 OpenAI 兼容请求 JSON

在发往 `POST {base}/chat/completions` **之前**，可将**与请求体一致的** JSON（`model`、`messages`、`stream`、`temperature`、`max_tokens`；千问/DeepSeek 扩展键在根级）打到 **info** 日志（stderr + 轮转日志文件）。

- **开启方式**（满足其一即可）：
  1. 与上文「LLM 请求落盘」相同：设置里打开「保存每轮对话请求」，或 `POINTER_DEBUG_LLM_PROMPTS=1`。
  2. **仅打日志、不落盘**：环境变量 **`POINTER_DEBUG_OPENAI_REQUEST=1`**（或 `true`）。
- **隐私与体积**：消息里过长的 `data:image/...` URL 会替换为占位说明；单条日志正文超过约 32KB 会截断并注明总长度。**不会**打印 `Authorization` 头或 API Key。

实现见 `crates/pointer-core/src/llm_prompt_dump.rs`（`try_log_openai_chat_request_json`），在 `provider.rs` 的 `chat_once` / `stream_chat` 中调用。

默认 **info** 不打思考字段。打开 **debug** 后，在 `POST …/chat/completions` 之前有一条 `openai_compat_request`：已展平到根级的最终请求体（含 `max_tokens`、`enable_thinking` / `reasoning_effort` 等），`messages` / `tools` 只记条数，无 API Key。

## 未纳入上下文的消息（仅调试）

开启「保存每轮对话请求」或 `POINTER_DEBUG_LLM_PROMPTS=1` 时，每轮 LLM 请求前会额外打 **info** 日志 `context_excluded_messages`：列出本会话中 `contextState.included=false` 的消息（`id`、`role`、`excludedReason`、内容预览）。界面不展示该标记。

实现见 `crates/pointer-core/src/message_context.rs`（`try_log_context_excluded_messages`）。

## 输出位置与格式

- 目录：`{应用数据目录}/logs/llm_prompts/{会话ID}/`
- 文件：`{unix_ms}_{uuid后16位}.json`
- 无会话 ID 的请求（连通性测试等）写入 `_unscoped` 子目录。
- 内容：包含时间戳、`conversationId`、阶段标签、模型名、流式/温度/max tokens、**合并后的扩展参数**（与线上一致，在根级），以及 `messages` 等；消息里过长的 `data:image/...` 会替换为占位说明以控制体积。

实现见 `crates/pointer-core/src/llm_prompt_dump.rs`。

## 助手流式原文（控制台，不经解析）

在 **`pointer-core` 的 OpenAI 流式路径** 中，可将模型增量按 **SSE 到达顺序** 连续写到 **stderr**（与结构化日志分流；内容为 API 返回的 UTF-8 片段原文，**不做** XML 解析）。

- **默认开启**。关闭：`POINTER_STREAM_RAW_LLM_TO_STDOUT=0`（或 `false`）。
- 写出形式：**仅纯文本拼接**。对每条非空的 `delta.content` / 推理增量（`delta.reasoning_content` 或 vLLM/Qwen 的 `delta.reasoning`）按顺序直接写入 stderr，**不加前缀、不打印序号、不在片段之间插入换行**（与模型在协议里给出的字节序一致，便于对照「推理与正文在同一 chunk 内先后出现」等现象）。
- 已不再提供回合结束时的整段 RAW 落盘环境变量；如需完整请求与消息体，请用本文开头的 **LLM 请求落盘**（`llm_prompt_dump`）。

实现见 `crates/pointer-core/src/provider.rs`（`stream_chat`、`write_llm_stream_chunk_to_stderr`）。

### 工具 JSON：流式解析（仅正文）

- **`reasoning_content`（及兼容 `reasoning`）与 `content` 分列缓冲**：推理只用于 UI / 历史；**只有 `content` 参与 JSON 工具信封**（流式渐进字段用 `partial-json-fixer` 修复后再解析），避免与 reasoning 交错。
- 流式过程中：用不完整 JSON 缓冲通过 `extract_json_streaming_partial` 推 `assistant_json_partial`；回合结束后再用 `finalize_json_tool_envelope` 得到完整工具列表，并可通过 `JsonToolStreamingReady` 推给前端（与 `StreamEvent` 定义一致）。
- **流结束后**对正文做严格 JSON 解析（失败则再尝试修复后解析）；若仍无可用 `tool_calls`，由上层按 `JsonToolFinishDiagnostics` 与既有重试提示处理。`merge_ui_order_reparse_ok` 字段保留在结构中，当前实现下恒为 `false`。
