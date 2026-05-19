# 模型扩展参数（千问 / DeepSeek）

应用在请求 OpenAI 兼容的 `POST …/chat/completions` 时，按服务商在设置里配置结构化参数；发往接口时写在**请求体根级**（千问、DeepSeek 均展平，无 `extra_body` 包裹）。

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

## 迁移

旧版 `extraBody` JSON、`thinkingEnabled` / `thinkingBudget` 在首次加载时会自动迁入上述字段；保存后不再写入 `extraBody`。

## 参考文档

- https://help.aliyun.com/zh/model-studio/deep-thinking
- https://docs.qwencloud.com/developer-guides/text-generation/thinking
