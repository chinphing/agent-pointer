# 右侧持久调试终端

右侧 Workspace Panel 的第一个主 Tab 是 **Terminal**，用于用户手工启动程序、持续查看日志和调试；它与 Agent 的 `terminal` 工具调用完全隔离。

## 会话模型

- 一个工作区可打开多个彼此独立的持久 PTY Shell 标签。
- 每个标签都有自己的 cwd、环境和前台进程；创建时默认从工作区目录启动。
- 标签只显示当前启动目录的名称；鼠标悬停显示完整绝对路径；右键可复制完整路径或关闭该 Shell。
- 关闭右侧栏或切到 Files / Changes 只卸载终端视图，不会结束已有 Shell；回到 Terminal 后继续使用同一批标签。
- 切换到另一个工作区时，旧工作区的标签会保留到显式关闭，避免意外中断正在运行的调试服务。
- 点击标签关闭、顶部“结束 Shell”或“重启 Shell”才会终止对应 session。
- Agent `terminal` 工具仍是一条命令一个子进程，不会写入、复用或中断用户控制台。

## 接口与事件

桌面端通过 Tauri command，Web 端通过同名语义的 HTTP API：

- 新建：`create_console_session` / `POST /api/console/sessions`
- 输入：`write_console_session` / `POST /api/console/sessions/:id/input`
- 调整尺寸：`resize_console_session` / `POST /api/console/sessions/:id/resize`
- 结束：`close_console_session` / `DELETE /api/console/sessions/:id`

创建接口返回 session id、工作区、cwd 和标签名。PTY 输出通过全局流事件 `console_output_delta` 传递；退出时发送 `console_session_exited`。

## 性能与包体

- `TerminalPanel.vue` 是异步组件，仅在切换到 Terminal Tab 时加载。
- xterm 与 fit addon 也使用动态 import；生产构建中它们是独立 chunk，不进入主聊天首屏 chunk。
- xterm scrollback 限制为 5,000 行；每个标签的前端输出缓冲最多保留 256 KiB，避免长期日志无限增长。

## 操作说明

1. 为当前会话选择项目目录。
2. 打开右侧栏的第一个 Terminal 图标，点击“新建终端”。
3. 需要多个环境或进程时，点击 `+` 新建独立 Shell 标签。
4. 在标签中执行 `npm run dev`、`cargo test` 等命令；使用 `Ctrl+C` 停止该标签的前台程序。
5. 鼠标悬停标签查看绝对路径；右键复制路径或关闭标签。
