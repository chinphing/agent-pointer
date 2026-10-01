# 工作区 HTML 预览

右侧工作区打开 `.html` / `.htm` 文本文件时，默认在沙箱 iframe 里渲染页面，而不是整份原文。

## 行为

- 工具栏 **原文** / **预览**（共用模式，见 [workspace-file-preview-mode.md](workspace-file-preview-mode.md)）
- 预览：沙箱 iframe（`allow-scripts allow-modals`，**没有** `allow-same-origin`）
- 页内 JS / CSS 会执行；脚本拿不到应用 cookie、DOM、本地存储
- 不要同时开 `allow-scripts` 和 `allow-same-origin`，否则页面可以拆掉沙箱
- 相对路径资源通常加载不到（独立源）；自包含页（内联 CSS/JS、`data:` 图）可完整预览
- 查找：预览里按源码文本计命中（跨源 iframe 无法高亮）；切到原文可看行内高亮
- 拖动工作区左边调整宽度时：盖住预览 iframe（避免页面抢走鼠标），并冻结 iframe 宽度（拖动结束再按新宽度排一次）。客户端与网页端相同。

客户端与网页端行为一致。

## 实现位置

- `src/lib/workspaceHtmlPreview.ts`
- `src/components/workspace/WorkspaceFilePreview.vue`
- `src/components/workspace/WorkspacePanel.vue`（拖动改宽时冻结 iframe）
