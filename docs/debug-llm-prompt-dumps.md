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
