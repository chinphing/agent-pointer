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

### 终端能力查询（OSC / DA / CPR）

控制台是「PTY 在后端、xterm 在前端」的拆分架构。若把 `OSC 10/11/12 ?`、
设备属性（`CSI c` / `CSI >c`）、光标位置（`CSI 6n`）原样转给 xterm，应答要再
经 IPC 写回 PTY，Ctrl+C 后常落到 shell 输入行（可见 `10;rgb:…` / `0;276;0c` 等）。

**方案**：在 `console_session` 的 PTY 读线程用 `TermQueryFilter`（`console_term_query.rs`）
拦截上述查询，**当场写回应答**，并不再转发到前端。这样查询/应答与 PTY 同进程，
避免异步往返。颜色默认与 Pointer 深色卡片主题一致；非查询类 OSC/CSI 仍原样上屏。

## 性能与包体

- `TerminalPanel.vue` 是异步组件，仅在切换到 Terminal Tab 时加载。
- xterm 与 fit addon 也使用动态 import；生产构建中它们是独立 chunk，不进入主聊天首屏 chunk。
- xterm scrollback 限制为 5,000 行；每个标签的前端输出缓冲最多保留 256 KiB，避免长期日志无限增长。
- **TUI（vim 等）**：`console_output_delta` 按 animation frame 合并后再更新 Vue/xterm，
  避免全屏重绘把 UI 打挂；按键写入按 session **串行**（避免 invoke/HTTP 乱序）。
- 截断缓冲时尽量落在换行处，减少半截 CSI 弄坏 alternate screen。

## 操作说明

1. 为当前会话选择项目目录。
2. 打开右侧栏的第一个 Terminal 图标，点击“新建终端”。
3. 需要多个环境或进程时，点击 `+` 新建独立 Shell 标签。
4. 在标签中执行 `npm run dev`、`cargo test` 等命令；使用 `Ctrl+C` 停止该标签的前台程序。
5. 鼠标悬停标签查看绝对路径；右键复制路径或关闭标签。

## 选中交互

截图里那种大块浅紫色矩形，是 xterm 的划词选区（`selectionBackground`），不是标签选中。

- 正常 **按住拖动划词**、双击选词、三击选行保持可用。
- 额外防护：若 `mouseup` 丢失（例如在 webview 外松开），xterm 仍挂着 `document mousemove`，会出现「点击后鼠标已松开，再移动仍继续选中」。检测到 `selectionPressing` 且 `buttons` 已无主键时，补发 `mouseup` 结束拖选。
- 开启 `macOptionClickForcesSelection`，避免 macOS 上 Option 进入 column-select。
- 标签栏使用 `user-select: none`，避免拖动时误选标签文字。

## macOS 中文输入与粘贴

桌面端（Tauri）在 macOS 上使用 WKWebView；Linux 部署的网页端通常是 Chromium。两者对 xterm 隐藏输入框的行为不同：

| 问题 | 原因 | 处理 |
| --- | --- | --- |
| 中文输入法候选框不出现 / 输入无效 | xterm helper textarea 默认 `opacity: 0` + `z-index: -5`，WebKit 不为其建立 IME | CSS/样式让 textarea 对引擎可见但内容透明（`TerminalPanel` + `terminalIme`） |
| 首键被吃掉 / 全角标点偶发丢失 | Safari IME 首键可能是 `keyCode 229` / `Process` / `Dead`；部分 `insertText` 在按键未抬起时被 xterm 丢弃 | `attachCustomKeyEventHandler` 只把真正的 IME 键交给浏览器（**不要**把裸 `keyCode 0` 当成 IME，否则 Esc/方向键/vim 无响应）；WebKit 下挂 `createTerminalImeGuard` 补发 |
| vim / TUI「无响应」 | 误 defer `keyCode 0`、每键 fire-and-forget 乱序、重绘洪泛 | 见上 + 输入串行 + 输出 rAF 合并；终端聚焦时不拦截 Ctrl+F（留给 vim） |
| `ls` 中文文件名变成 `?`（英文正常） | Dock 启动的 GUI 进程常带 `LANG=C`；**真 PTY** 下 macOS `ls` 会把非 ASCII 打成 `?`（不是缺字体） | `unix_locale::apply_unix_utf8_child_env`：终端子进程补齐/纠正为 UTF-8 `LANG`（优先系统 `AppleLocale`） |
| 中文粘贴/字形缺字 | 等宽字体缺 CJK 时依赖系统中文 UI 字体回退 | `terminalFontFamily()`：等宽在前，系统默认中文在后（macOS 苹方 / Windows 雅黑）；**不要**把比例中文字体放到栈首 |

网页端 Chromium 不启用 IME guard，避免重复投递。后端 PTY 写路径本身支持 UTF-8（见 `console_session` 往返测试）。
