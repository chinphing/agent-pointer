# 模型扩展参数（`extra_body`）

应用在请求 OpenAI 兼容的 `POST …/chat/completions` 时，可将可选的 **`extra_body`** 以 **JSON 对象**形式放在请求体顶层（字段名 `extra_body`），供百炼、Qwen 等网关解析（例如 `enable_thinking`、`thinking_budget` 等，以各服务商文档为准）。

## 配置方式

- **服务商**：`settings.json` 里该服务商的 **`extraBody`**（对象）。
- **按模型**：`modelConfigs[模型 id].extraBody`（对象）。
- **合并规则**：当前选中模型对应的 `extraBody` 与服务商默认 **浅合并**，**同名键以模型为准**；任一侧未配置则只用另一侧；都未配置则不发送 `extra_body`。

旧版若仅有 `thinkingEnabled` / `thinkingBudget`，首次加载时会自动合并进 `extraBody` 后再使用；保存时会写入新的 `extraBody` 字段。

## 参考文档

- https://help.aliyun.com/zh/model-studio/deep-thinking
- https://docs.qwencloud.com/developer-guides/text-generation/thinking

其他兼容网关若不支持 `extra_body` 或内部键名不同，请按其文档调整 JSON 或留空。
