# 工作区文本预览查找

右侧工作区打开**文本文件预览**时，支持常规查找（与对话内 ⌘/Ctrl+F 互不抢占）。

## 行为

- 工具栏「查找」按钮，或焦点在工作区面板内时按 **⌘F / Ctrl+F**
- 输入关键词（不区分大小写），显示 `当前/总数`
- **Enter** 下一个、**Shift+Enter** 上一个、**Esc** 关闭
- 源码视图：高亮每次出现并滚动到当前项
- Markdown 预览：在渲染正文中高亮并导航
- 图片 / PDF / 二进制预览不提供查找

## 快捷键优先级

文件预览在 `window` **捕获阶段**认领 ⌘/Ctrl+F；对话页查找仅在事件未被认领时打开。

## 实现位置

- `src/components/workspace/WorkspaceFilePreview.vue`
- `src/lib/workspaceFilePreview.ts`（`findFilePreviewMatches` / `filePreviewSearchParts`）
