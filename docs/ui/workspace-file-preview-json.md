# 工作区 JSON 预览

右侧工作区打开 `.json` / `.jsonc` 文本文件时，默认用可折叠树展示，而不是整份原文。

## 行为

- 工具栏 **原文** / **预览**（共用模式，见 [workspace-file-preview-mode.md](workspace-file-preview-mode.md)）
- 预览：对象、数组可展开/折叠；折叠时显示 `{n}` / `[n]`
- 默认展开前两层，更深层收起
- 非法 JSON、含注释的 JSONC、截断后无法解析、或节点过多：只显示原文（无预览切换）
- 查找：在预览里搜键名和值；命中会展开祖先并滚动到当前项

客户端与网页端行为一致。

## 实现位置

- `src/lib/workspaceJsonPreview.ts`
- `src/components/workspace/WorkspaceJsonTree.vue`
- `src/components/workspace/WorkspaceFilePreview.vue`
