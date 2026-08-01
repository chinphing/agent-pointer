# 模型扩展参数（千问 / DeepSeek / extra_body）

应用在请求 OpenAI 兼容的 `POST …/chat/completions` 时，按服务商在设置里配置参数；
发往接口时写在**请求体根级**（与 `temperature` 同级，无 `extra_body` 包裹），对齐 Hermes /
OpenAI Python SDK 的展平行为。

## 千问（DashScope 兼容）

| 设置项 | 磁盘字段 | 请求体字段 | 说明 |
|--------|----------|------------|------|
| 深度思考 | `enableThinking` | `enable_thinking` | 是 / 否 |
| 思考预算 | `thinkingBudget` | `thinking_budget` | 仅当深度思考为「是」时发送；默认 **2048** |

可在**服务商默认**或**各模型**单独配置；模型未设置时继承服务商默认。

**创造性**（`temperature`）、**最大输出**（`maxTokens`）同样在服务商级可设默认，模型「定制」可覆盖。

## DeepSeek

| 设置项 | 磁盘字段 | 请求体字段 | 取值 |
|--------|----------|------------|------|
| 推理力度 | `reasoningEffort` | `reasoning_effort` | `high`（高力度）、`max`（最大力度） |

可在服务商或各模型配置；模型可「跟随服务商默认」。

## 自由扩展参数 `extraBody`（对齐 Hermes）

调试模式 → 模型服务 → 服务商 / 模型定制中的 **扩展参数 (extra_body)**：

```json
{ "repetition_penalty": 1.1, "top_p": 0.8 }
```

- 磁盘字段：`providers[].extraBody`，模型覆盖为 `modelConfigs[model].extraBody`
- 合并顺序（后者覆盖前者）：服务商 `extraBody` → 模型 `extraBody` → 上方结构化字段
- 上线时展平到请求体根级，例如：

```json
{
  "model": "Qwen3.6-27B-AWQ-INT4",
  "temperature": 0.7,
  "repetition_penalty": 1.1,
  "top_p": 0.8
}
```

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

- `extra_body` → 服务商级 `ProviderConfig.extraBody`
- `model_extra_body.<model>` → 该模型 `modelConfigs` 覆盖（浅合并时覆盖服务商同名键）

## 迁移

旧版仅含 thinking 的 `extraBody` JSON、`thinkingEnabled` / `thinkingBudget` 在加载时会迁入
结构化字段；其余未知键保留为新的 `extraBody`。

## 参考文档

- https://help.aliyun.com/zh/model-studio/deep-thinking
- https://docs.qwencloud.com/developer-guides/text-generation/thinking
