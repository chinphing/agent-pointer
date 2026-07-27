# 流式 Markdown 渲染节流

流式输出时 Markdown 解析较贵，前端用 `useThrottledMarkdown` 合并刷新，而不是每个 token 立刻重渲染。

## 间隔

| 条件 | 间隔 | 常量 |
|------|------|------|
| 流式中，正文长度 ≤ 8000 | **100ms** | `STREAMING_MARKDOWN_THROTTLE_MS` |
| 流式中，正文长度 > 8000 | **250ms** | `LONG_STREAMING_MARKDOWN_THROTTLE_MS` |
| 流式结束 / 非流式 | 立即解析 | — |

长度阈值：`LONG_SOURCE_THRESHOLD = 8000`。

实现：`src/composables/useThrottledMarkdown.ts`。
