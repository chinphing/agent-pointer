# 工作区原文 / 预览模式

文本文件预览共用一套 **原文 / 预览** 状态，而不是按格式各写一份开关。

图片、PDF、二进制走媒体/占位路径，不进入这套模式。

## 行为

- 一种格式只注册一次：扩展名 → 预览类型
- 工具栏只有一组 **原文 / 预览**；当前文件没有可用预览时不显示
- 默认预览；切文件保留当前选择（组件未销毁时）
- 预览构建失败（如非法 JSON）时只显示原文

## 加一种格式

1. 在 `WORKSPACE_RICH_PREVIEW_DEFS` 登记扩展名，以及是否总是可预览
2. 若需解析后才能预览，在 `WorkspaceFilePreview` 里补内容就绪判断
3. 模板按 `textSurface` 增加渲染分支
4. 若预览里的查找与原文不同，再补查找

客户端与网页端同一套逻辑。

## 实现位置

- `src/lib/workspacePreviewMode.ts`（注册表、扩展名、当前应显示原文还是哪种预览）
- `src/components/workspace/WorkspaceFilePreview.vue`（单一 `viewMode`、工具栏、按 surface 渲染）
- 各格式渲染：Markdown 在预览组件内；JSON 见 [workspace-file-preview-json.md](workspace-file-preview-json.md)
