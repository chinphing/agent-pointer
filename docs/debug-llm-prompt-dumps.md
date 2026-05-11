# LLM 请求落盘调试

用于排查「每一轮实际发给模型的内容」时，可开启将请求体写入本地 JSON 文件。

## 开启方式

1. **设置**：在「生成参数」中打开「保存每轮对话请求」（对应 `ModelSettings.debugDumpLlmPrompts` / JSON 字段 `debugDumpLlmPrompts`）。
2. **环境变量**（不依赖设置项）：`POINTER_DEBUG_LLM_PROMPTS` 为 `1` 或 `true` 时强制开启。

## 输出位置与格式

- 目录：`{应用数据目录}/logs/llm_prompts/`
- 文件：`{unix_ms}_{uuid}.json`
- 内容：包含时间戳、阶段标签、模型名、流式/温度/max tokens，以及 `messages` 等；消息里过长的 `data:image/...` 会替换为占位说明以控制体积。

实现见 `crates/pointer-core/src/llm_prompt_dump.rs`。

## 助手流式原文（控制台，不经解析）

在 **`pointer-core` 的 OpenAI 流式路径** 中，可将模型增量按 **SSE 到达顺序** 连续写到 **stderr**（与结构化日志分流；内容为 API 返回的 UTF-8 片段原文，**不做** XML 解析）。

- **默认开启**。关闭：`POINTER_STREAM_RAW_LLM_TO_STDOUT=0`（或 `false`）。
- 写出形式：**仅纯文本拼接**。对每条非空的 `delta.content` / `delta.reasoning_content` 按顺序直接写入 stderr，**不加前缀、不打印序号、不在片段之间插入换行**（与模型在协议里给出的字节序一致，便于对照「推理与正文在同一 chunk 内先后出现」等现象）。
- 已不再提供回合结束时的整段 RAW 落盘环境变量；如需完整请求与消息体，请用本文开头的 **LLM 请求落盘**（`llm_prompt_dump`）。

实现见 `crates/pointer-core/src/provider.rs`（`stream_chat`、`write_llm_stream_chunk_to_stderr`）。

### 工具 XML：流式解析（仅正文）

- **`reasoning_content` 与 `content` 分列缓冲**：推理只用于 UI / 历史；**只有 `content` 会进入 `XmlToolParser`**，避免同一缓冲内与 reasoning 交错导致标签被截断（例如 `<thoughts>` 被拆成错误 token）。
- 流式过程中：完整闭合一段 `<response>…</response>` 后会通过 `XmlToolStreamingReady` 等路径推给前端；尚未闭合时，可从当前正文缓冲提取已闭合子标签，通过 `AssistantXmlPartial` 渐进更新（与 `StreamEvent` 定义一致）。
- **流结束后不再做**「仅正文重放 / 仅推理重放 / 推理+正文合并」等二次解析兜底；若回合结束仍无可用 `tool_calls`，由上层按 `XmlToolFinishDiagnostics` 与既有重试提示处理。`merge_ui_order_reparse_ok` 字段保留在结构中，当前实现下恒为 `false`。
