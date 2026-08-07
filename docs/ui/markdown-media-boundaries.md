# Markdown / SVG / Chart / HTML 功能边界

聊天与 Markdown 预览以 **Markdown 为主通道**。SVG、Chart、受限 HTML 都是正文里的特化能力，不是并列的三种文档格式。

## 选用表

| 需求 | 用法 |
|------|------|
| 叙述、列表、精确多列表格、可复制数据 | **Markdown**（GFM 表优先） |
| 长单元格换行 | GFM 表单元格内用 `<br>`，或 HTML 表 |
| 流程 / 架构 / 决策示意 | ` ```svg ` fence |
| 数值对比 / 趋势 | ` ```chartjs ` / ` ```chart ` fence |
| **固定列宽**、结果着色 | ` ```html ` fence（推荐）或正文裸 `<table>` |
| 整页布局、脚本、外链小应用 | **不支持** |

## 各通道边界

### Markdown（默认）

- 正文结构与可读内容。
- GFM 表由宿主包一层 `.table-wrapper` 并套主题单元格样式。
- IM 通道一般能较好展示 Markdown；复杂 HTML 不可靠。

### ` ```svg `

- 仅此 fence 承载矢量图；正文裸 `<svg>` 不应作为正式画图通道。
- 专用消毒、流式占位、卡片工具条。见 [markdown-svg.md](markdown-svg.md)。

### ` ```chartjs ` / ` ```chart `

- 仅 JSON 配置，本地 Chart.js 渲染。见 [markdown-charts.md](markdown-charts.md)。

### ` ```html ` / ` ```htm `（推荐写 HTML 表）

模型常用 ` ```html ` 包住表格。宿主会把该 fence **渲染成 HTML**（不是代码卡片）：

1. 轻量消毒（去掉 `script` / `iframe` / 事件属性等）；
2. `<table>` 包进 `.table-wrapper`；
3. 主题 `th`/`td` 样式；`<colgroup>` / `%` 列宽用 `table-layout: fixed`；
4. 保留单元格内联颜色等 `style`。

正文里**不包 fence**的裸 `<table>` 同样会渲染（同上 2–4）。
解析前会在 HTML 表前后补空行，避免 CommonMark 把后面的 `##` / 列表 / 链接当成纯文本。

**约定：**

- 需要定宽时用 ` ```html ` + `<colgroup>`，不要用 GFM 表硬撑。
- 不要用 HTML 画流程图（用 `svg`）或图表（用 `chartjs`）。
- **飞书 / 企微等 IM**：HTML 表不可靠，重要数据另给 GFM 表或短摘要。

**示例：**

````md
```html
<table>
  <colgroup>
    <col style="width:8%">
    <col style="width:12%">
    <col style="width:50%">
    <col style="width:15%">
    <col style="width:15%">
  </colgroup>
  <tr><th>#</th><th>Name</th><th>Description</th><th>Amount</th><th>Status</th></tr>
  <tr>
    <td>1</td>
    <td>张三</td>
    <td>赴北京参加全国医院基建项目评审会议…</td>
    <td>1,280.50</td>
    <td style="color:#1a7f37;">通过</td>
  </tr>
</table>
```
````

## 一句话

- **Markdown** = 内容与结构  
- **SVG fence** = 矢量图示  
- **Chart fence** = 数据可视化  
- **`html` fence** = 定宽 / 着色表格等排版补丁  

实现：[`src/lib/markdownConfig.ts`](../../src/lib/markdownConfig.ts)、[`src/lib/markdownHtml.ts`](../../src/lib/markdownHtml.ts)、[`src/styles/globals.css`](../../src/styles/globals.css)。
