# Markdown 正文排版

聊天气泡与工作区 `.md` 预览共用 `.md-body`（`src/styles/globals.css`）。改排版只动这一套样式，不要在组件里再写一套字号。

对话列宽仍跟中间栏走（上限 1024px），见 [visual-theme.md](visual-theme.md)。不要给 `.md-body` 再加一层正文 `max-width`：表格、代码、图、Chart 要吃满列宽。

## 层级

| 元素 | 表现 |
|------|------|
| 正文 | 15px，行高 1.75 |
| `h1`–`h6` | 字号用 `em`（工具行 `text-[12px]` 等宿主会跟着缩小）；`h1` 最大，`h4+` 与正文同字号、仅加粗 |
| 单独一行的 `**标题**` | 解析成 `p.md-section-title`，按小节标题加字号 |
| 气泡首行单独加粗 | 再大一档，接近 `h1` |
| `**条目** — 说明` | 解析成 `.md-lead`，加粗段单独成行（`：` / `:` / 破折号同理） |

分类在 `classifyMarkdownParagraph`（`markdownConfig.ts`）。`**口径提醒：**后文` 这种冒号写在加粗里的，保持一行，不当标题。
| 引用 | 左 2px `border`，正文用 `--muted` |
| 分隔线 | 1px `--border`，上下留白 |

字号不要写死 `px`，也不要给标题上强调色。

## 列表与段落

- 段距约 `0.7em`，列表项之间 `0.4em`
- 嵌套列表略收紧，避免和工具卡片抢位置
- `.md-body-flow` 仍去掉首尾外边距，避免气泡上下空一截

## 不要改的

- 行内 code：强调色、无底（见 [visual-theme.md](visual-theme.md)）
- 围栏代码 / 表 / Chart / Mermaid / SVG 卡片 chrome
- 对话列 `max-w` 与 gutter

## 平台

同一套 CSS：桌面（WKWebView / WebView2 / WebKitGTK）与 `web:dev`。`:has()`、`text-wrap: balance` 在当前壳里可用；不要为旧内核再写一套。
