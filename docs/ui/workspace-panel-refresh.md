# 右侧工作区面板刷新约定

实现：`src/components/workspace/WorkspacePanel.vue`（由 `AppShell` 传入 `workspaceRoot` + `conversationId`）。

## 何时会刷新

| 场景 | 行为 |
|------|------|
| 打开面板 | 先切换 UI，后台加载文件树 + `git status`（供顶栏变更数字） |
| 切换工作区根 | 清空预览 Tab，回到「工作区文件」；后台重载目录与变更数 |
| 切换会话（同工作区） | 丢弃其他会话的「本轮 Diff」Tab；**静默**后台刷新当前视图 |
| 点文件夹 / Git 主导航 | **先切换 Tab**，再后台 silent 拉取（保留旧列表直到新数据到达） |
| 点已有预览 Tab / 再次点同一文件 | **只切换激活态**，不重读内容（避免闪 loading） |
| 工具栏刷新按钮 / 右键「刷新标签」 | 后台刷新当前视图（可显示 loading） |

顶栏 Git 图标上的数字来自 `changes.length`，不再要求先点进「变更文件」才出现。

交互原则：点击路径不 `await` IO；列表用 stale-while-revalidate；文件树 / git status 用序号丢弃过期结果。

## 刻意保留

- 同工作区下的文件预览、Git Diff Tab 在换会话后可保留（路径仍有效），但内容会在再次激活时重载。
- 会话 hydration 短暂出现空 `workspaceRoot` 时不清空 Tab，避免闪断。

## 右键选中

右键打开菜单时，文件树 / 变更列表 / 预览 Tab 对应行保持整行高亮（`is-selected` / `is-context-selected`），直到菜单关闭。菜单遮罩会吃掉 hover，因此不能只依赖 `:hover`。

## 右键删除

文件树右键提供 **删除**（文件 / 文件夹 / 符号链接）。桌面 WebView 无可靠 `window.confirm`，因此走确认弹层后再调用：

- 桌面：`delete_workspace_path`（Tauri）
- Web：`DELETE /api/workspace/path`

后端只允许删除工作区内相对路径，禁止删除工作区根与 `..` 逃逸。成功后关闭相关预览 Tab，并从文件树就地移除节点（保留已展开目录）；同时静默刷新 Git 变更角标。

## 跨端

桌面与 Web 共用同一组件与 API；两端行为一致。
