# 模型扩展参数（思考强度策略 / extra_body）

应用在请求 OpenAI 兼容的 `POST …/chat/completions` 时，按服务商在设置里配置参数；
发往接口时写在**请求体根级**（与 `temperature` 同级，无 `extra_body` 包裹），对齐 Hermes /
OpenAI Python SDK 的展平行为。

## 策略模式

产品侧统一「思考强度」：`off | low | medium | high | max`（磁盘字段 `thinkingIntensity`）。
线路字段由策略（`thinkingProtocol`）翻译，实现见 `thinking_strategy` 模块：

| Protocol | 线路字段 | 典型厂商 |
|----------|----------|----------|
| `budget` | `enable_thinking` + `thinking_budget` | 千问 / DashScope |
| `effort` | 仅 `reasoning_effort`=`low`/`high`/`max` | DeepSeek V4 |
| `openrouter` | `reasoning: { enabled, effort }` | OpenRouter |
| `kimi` | `thinking` **xor** `reasoning_effort` | Kimi / Moonshot |
| `openai_effort` | 顶层 `reasoning_effort`（含 `none`/`xhigh`） | 智谱 / GPT 风格 |
| `off` | 不发结构化思考字段 | 豆包等 |
| `auto` | 按服务商 URL / id 推断 | 默认 |
| `custom` | 仅 `extraBody`，忽略强度 | 自建端点 |

强度解析顺序：本轮 `round_thinking_intensity`（场景档位选出的强度）→
未走档位映射时再读模型/服务商 `thinkingIntensity` →
遗留 `reasoningEffort` / `enableThinking` + `thinkingBudget`。
档位映射一旦套用，不再回读模型目录默认。

设置界面与平台目录都只选「思考强度」。
`thinkingProtocol` 由客户端按服务商 URL / id 推断，官网不下发、不转换。
自定义 OpenAI 兼容端点在 `auto` 下按 `openai_effort` 翻译强度。

采样参数：`temperature` 默认 **0.7**，`top_p` 默认 **0.95**。
可在平台目录（服务商默认或模型覆盖）与客户端模型服务里配置；发往 chat/completions 时写在请求体根级。

发往 OpenAI 兼容接口前打 **debug** `openai_compat_request`：展平后的最终根级字段
（`max_tokens` 与线路思考键），不含 `messages` / Key。
完整请求体仍走「保存每轮对话请求」或 `POINTER_DEBUG_LLM_PROMPTS`。

## 千问（budget）

| 设置项 | 磁盘字段 | 请求体字段 | 说明 |
|--------|----------|------------|------|
| 深度思考 | `enableThinking` | `enable_thinking` | 是 / 否 |
| 思考预算 | `thinkingBudget` | `thinking_budget` | 仅当深度思考为「是」时发送；默认 **2048** |

强度档位映射预算：low→1024，medium→2048，high→4096，max→8192。

## DeepSeek（effort）

官方只认这两种等价写法之一，取值只有 `low` / `high` / `max`（无 `medium`，无 `thinking.type`）：

- `{"reasoning_effort": "low|high|max"}`
- `{"output_config": {"effort": "low|high|max"}}`

本实现只发第一种。产品档位 `medium` 落到 `high`。关闭思考时不发上述字段。

历史里的 **assistant** 必须带回上一轮的 `reasoning_content`。Run 内压缩摘要也是 assistant，
主机写入时带合成 reasoning；组请求时若该字段为空会补上，**不会**改成 `user`。

| 设置项 | 磁盘字段 | 请求体字段 | 取值 |
|--------|----------|------------|------|
| 思考强度 | `thinkingIntensity` / `reasoningEffort` | `reasoning_effort` | `low` / `high` / `max` |

## OpenRouter / Kimi / OpenAI 力度

- OpenRouter：`reasoning.enabled` + `reasoning.effort`
- Kimi：开启思考用 `thinking`；力度用 `reasoning_effort`（二者不并存）
- openai_effort：顶层 `reasoning_effort`（off→`none`，max→`xhigh`）

## 强制关闭思考（摘要压缩等）

`chat_once_without_thinking` 与带 `response_format` 的结构化调用会通过
`apply_thinking_disabled_strategy` 按当前策略关思考：

| Protocol | 强制关闭时写入 |
|----------|----------------|
| `budget` | `enable_thinking=false`，去掉 `thinking_budget` |
| `effort` | 去掉 `reasoning_effort` / `thinking` / `output_config` |
| `openrouter` | `reasoning.enabled=false` |
| `kimi` / `openai_effort` | 去掉或置 `none` |

## 自由扩展参数 `extraBody`（对齐 Hermes）

调试模式 → 模型服务 → 服务商 / 模型定制中的 **扩展参数 (extra_body)**：

```json
{ "repetition_penalty": 1.1, "top_p": 0.8 }
```

- 磁盘字段：`providers[].extraBody`，模型覆盖为 `modelConfigs[model].extraBody`
- 合并顺序（后者覆盖前者）：服务商 `extraBody` → 模型 `extraBody` → 上方结构化字段
- 上线时展平到请求体根级

嵌套对象按服务商文档原样书写（例如部分 vLLM / NIM 的 `chat_template_kwargs`）。

### pointer-server.toml

独立部署可在 `[llm.providers.<id>]` 直接写（启动注入内存平台配置）：

```toml
[llm.providers.local]
api_key = "no-key"
base_url = "http://127.0.0.1:8000/v1"
models = ["Qwen3.6-27B-AWQ-INT4"]
extra_body = { repetition_penalty = 1.1, top_p = 0.8 }

[llm.providers.local.model_extra_body."Qwen3.6-27B-AWQ-INT4"]
top_p = 0.9
```

## 迁移

旧版仅含 thinking 的 `extraBody` JSON、`thinkingEnabled` / `thinkingBudget` 在加载时会迁入
结构化字段；其余未知键保留为新的 `extraBody`。

## 参考文档

- https://help.aliyun.com/zh/model-studio/deep-thinking
- https://docs.qwencloud.com/developer-guides/text-generation/thinking
