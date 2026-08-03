# 回复富媒体提示词（Charts / SVG）

Chart.js 与 SVG 示意图的模型侧说明从各 Agent `AGENT.md` 正文剥离，放到共享切片，便于按需组装。

| 切片 | 路径 | 注入入口 |
|------|------|----------|
| Charts（含 App/IM 交付行为） | [`crates/pointer-core/src/agents/_shared/CHARTS.md`](../../crates/pointer-core/src/agents/_shared/CHARTS.md) | `rendered_charts_inject()` |
| SVG diagrams（含 App/IM 行为） | [`crates/pointer-core/src/agents/_shared/SVG_DIAGRAMS.md`](../../crates/pointer-core/src/agents/_shared/SVG_DIAGRAMS.md) | `rendered_svg_diagrams_inject()` |

`MEDIA_DELIVERY` 只负责本地文件 `MEDIA:` 交付，不再写 Charts / SVG 说明。

在 `push_agent_role_cacheable_prompts` 中：仅 **`AgentProfile::General` / `Coder` / `Computer`**（主会话或子 Agent）注入：

`COMMUNICATION_PUBLIC` →（general/coder/computer）`MEDIA_DELIVERY` → `CHARTS` → `SVG_DIAGRAMS` → Agent 系统提示 → …

其它 profile（explore / research / supervisor 等）只注入 `COMMUNICATION_PUBLIC` + 自身系统提示，不带这三块。

约定：提示词英文、短行、不写仓库文件名；产品行为见 [`../ui/markdown-charts.md`](../ui/markdown-charts.md)、[`../ui/markdown-svg.md`](../ui/markdown-svg.md)。组装总序见 [`../internals/llm-prompt-assembly-order.md`](../internals/llm-prompt-assembly-order.md)。
