# 右侧工作区面板刷新约定

实现：`src/components/workspace/WorkspacePanel.vue`（由 `AppShell` 传入 `workspaceRoot` + `conversationId`）。

## 何时会刷新

| 场景 | 行为 |
|------|------|
| 打开面板 | `v-if` 挂载组件，立即加载文件树，并**后台**拉 `git status`（供顶栏变更数字） |
| 切换工作区根 | 清空预览 Tab / Git 列表，回到「工作区文件」并重载；同步后台刷新变更数 |
| 切换会话（同工作区） | 丢弃其他会话的「本轮 Diff」Tab；重载当前主视图或预览内容，并刷新变更数 |
| 点文件夹 / Git 主导航 | **始终**重新拉取目录或 `git status`（不只在空列表时）；在文件树时也会后台刷新变更数 |
| 点已有预览 Tab / 再次点同一文件 | 重新读文件或重算 Diff，并后台刷新变更数 |
| 工具栏刷新按钮 | 刷新当前激活视图（含变更数字） |

顶栏 Git 图标上的数字来自 `changes.length`，不再要求先点进「变更文件」才出现。后台刷新使用 silent 模式，避免把数字先清成空。

## 刻意保留

- 同工作区下的文件预览、Git Diff Tab 在换会话后可保留（路径仍有效），但内容会在再次激活时重载。
- 会话 hydration 短暂出现空 `workspaceRoot` 时不清空 Tab，避免闪断。

## 右键选中

右键打开菜单时，文件树 / 变更列表 / 预览 Tab 对应行保持整行高亮（`is-selected` / `is-context-selected`），直到菜单关闭。菜单遮罩会吃掉 hover，因此不能只依赖 `:hover`。

## 跨端

桌面与 Web 共用同一组件与 API；两端行为一致。
