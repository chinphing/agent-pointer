# 右侧工作区面板刷新约定

实现：`src/components/workspace/WorkspacePanel.vue`（由 `AppShell` 传入 `workspaceRoot` + `conversationId`）。

## 何时会刷新

| 场景 | 行为 |
|------|------|
| 打开面板 | 先切换 UI，后台加载文件树 + `git status`（供顶栏变更数字） |
| 切换工作区根 | 清空预览 Tab，回到「工作区文件」；后台重载目录与变更数 |
| 切换会话（同工作区） | 丢弃其他会话的「本轮 Diff」Tab；**静默**后台刷新当前视图 |
| 点文件夹 / Git 主导航 | **先切换 Tab**，再后台 silent 拉取（保留旧列表直到新数据到达） |
| 点已有预览 Tab / 再次点同一文件 | **先激活 Tab**，再后台重载内容 |
| 工具栏刷新按钮 | 后台刷新当前视图（可显示 loading） |

顶栏 Git 图标上的数字来自 `changes.length`，不再要求先点进「变更文件」才出现。

交互原则：点击路径不 `await` IO；列表用 stale-while-revalidate；文件树 / git status 用序号丢弃过期结果。

## 刻意保留

- 同工作区下的文件预览、Git Diff Tab 在换会话后可保留（路径仍有效），但内容会在再次激活时重载。
- 会话 hydration 短暂出现空 `workspaceRoot` 时不清空 Tab，避免闪断。

## 右键选中

右键打开菜单时，文件树 / 变更列表 / 预览 Tab 对应行保持整行高亮（`is-selected` / `is-context-selected`），直到菜单关闭。菜单遮罩会吃掉 hover，因此不能只依赖 `:hover`。

## 跨端

桌面与 Web 共用同一组件与 API；两端行为一致。
