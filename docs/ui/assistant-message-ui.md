# 助手消息 UI：`thoughts` 与 API `reasoning`

本文约定 **主气泡展示**、**竖线进度**、**原始输出调试面板** 的分工，避免后续改动把 `reasoning` 当成主界面正文或把竖线改成条形进度条/「仅可见通道」而偏离产品意图。

## 两个通道（勿混用）

| 字段 | 来源 | 主气泡（`AssistantModelMessage`） |
|------|------|-------------------------------------|
| **`thoughts`** | 正文 JSON 对象中的 `thoughts` 字符串（解析后写入 `message.thoughts`） | **直接展示**：`ModelThoughtPanels` 在正文 Markdown 上方展示灰字 `thoughts`。 |
| **`reasoning`** | 兼容 OpenAI 的 **`reasoning_content`** 增量（与 `content` 分流） | **不展示正文**：不得把 `reasoning` 拼进 `message.content` 或主区 Markdown；仅用于持久化/API 回传（见设置 `reasoningInMessages`）与下调试。 |

## 「原始输出」（代码图标）

- 入口：助手消息完成后，复制按钮旁的 **代码图标**（受设置 `rawContentViewEnabled` 控制是否显示入口）。
- 面板：`RawWirePanel`，合并展示 **推理（若有）** 与 **正文通道原始字串**（`rawContent`），供调试与复制。
- **`reasoning` 只应出现在此面板内**，不要改到主气泡流式区域。

## 无 `headline` 时的竖线进度（`|`）

- 实现：`AssistantModelMessage.vue` 中 **`streamedCharCount`**。
- **必须**取以下各字段长度的 **最大值**（反映「整条流式输出」体量，含用户看不见的 reasoning 通道）：
  - `content`
  - `rawContent`
  - `thoughts`
  - `toolNamePreview`
  - `responseTextDraft`（`response` 的 `tool_args.text` 流式预览）
  - **`reasoning`**
- **常见错误**：为「主界面可见」而从 `streamedCharCount` 里去掉 `reasoning` —— 会导致竖线在模型大量输出 reasoning、正文尚未跟进时几乎不动，与「整段输出流」语义不符。
- **与上文的边界**：竖线可随 `reasoning` **长度**增长；**仍不得**在主气泡里渲染 `reasoning` 文本。

## 后端与前端事件（便于对照）

- 后端对 `reasoning_content` 仍发 `reasoning_delta`；前端 `chat` store 累积到 `message.reasoning`（与是否写入下一轮 API 的设置解耦时，以当前代码为准）。
- 流式阶段若 `content` 以 `{` 开头（`json_object` 工具信封），主气泡 **不** 把该通道当 Markdown 渲染；`response` 时由 `assistant_json_partial.responseText` 写入 **`responseTextDraft`**，主区用其做 Markdown 流式展示。`thoughts` / `headline` 仍由同事件更新。回合结束 `message_end` 后 `content` 会替换为 `extract_user_visible_content` 结果，并清除 `responseTextDraft`。

## 子 Agent / Supervisor 进度（`AgentProgressTimeline`）

- 数据：`agent_step` 流事件 → `message.agentTrace`；规划列表 → `supervisor_plan` → `message.supervisorPlanTasks`。
- 状态映射：`planning` / `running` / `completed` / `failed` / `summarizing` → 图标与中文标签。
- 无 `headline` 时竖线数量仍由 **`streamedCharCount`** 驱动（含 **`reasoning`** 长度），见上文竖线节；**UI 为逐段增加的 `|` 字符，不是条形进度条**。

修改 `AssistantModelMessage.vue`、`RawWirePanel.vue`、`ModelThoughtPanels.vue`、`AgentProgressTimeline.vue` 或 `reasoning_delta` 处理前，请先对照本文。
