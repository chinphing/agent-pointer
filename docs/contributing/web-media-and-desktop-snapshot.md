# Web 附件与桌面截图

## 附件（Web 与桌面）

Composer 支持三种添加方式：回形针选择、粘贴图片、拖入文件（拖入区域为输入框面板）。

| 端 | 拖入实现 |
|----|----------|
| **Web** | 标准 HTML5 `drop`（`capture` 监听，覆盖输入框面板） |
| **桌面 (Tauri)** | `webview.onDragDropEvent` 取本地路径。Tauri 拦截 OS 文件拖放，事件为**整窗**级别；不做坐标命中（frameless/overlay 窗口坐标不可靠，见 [tauri#10744](https://github.com/tauri-apps/tauri/issues/10744)），窗口内任意位置放下文件即添加到 Composer |

桌面端走本地路径读取（视频等大文件与文件选择器一致）。macOS / Windows / Linux 三端行为一致；Web 端通过浏览器 `File` API 读取内容。

### 维护易错点（Composer 拖入附件）

修改 `Composer.vue` 或 `tauri.conf.json` 前请读 `Composer.vue` 内 **Composer file drag-and-drop** 注释块。以下为曾反复踩坑、勿再改错的约定：

| 勿做 | 原因 |
|------|------|
| 主窗口设置 `dragDropEnabled: false` | 与 Tauri 原生 OS 拖放互斥；macOS 上 HTML5 `@drop` 对 Finder 文件常不触发 |
| 用 `onDragDropEvent` 的 `position` + `getBoundingClientRect` 做落点命中 | 坐标相对窗口外框，与 viewport 不一致（overlay 标题栏约 28px 量级偏差） |
| 桌面端仅依赖模板 `@drop` 收文件 | OS 文件拖入时 WebView 不派发 HTML5 drop，必须用 `onDragDropEvent` |
| 去掉 HTML5 处理器里的 `if (isTauriRuntime()) return` | 标明 Web/Tauri 双路径；避免误以为桌面走 DOM drop |
| 调用 `webview.scaleFactor()` | Tauri 2 上在 `Window` 上，不在 `Webview` |
| 改 `tauri.conf` 后只热更新前端 | `dragDropEnabled` 等在窗口创建时生效，需完整重启 `tauri dev` / 重装包 |

实现位置：`src/components/chat/Composer.vue`（`setupTauriComposerDragDrop` + Web `@drop`）；主窗口 `drag_drop_enabled` 见 `src-tauri/tauri.conf.json`（默认 `true`，勿随意改 false）。

| 能力 | 桌面 (Tauri) | Web (pointer-server) |
|------|--------------|----------------------|
| 图片/音频预览 | `previewChatMedia` | 同左（JSON base64 API） |
| 视频预览 | `convertFileSrc` | `GET /api/chat/media-stream?storageRelPath=…` |
| 文件打开/下载 | OS 默认应用 | `GET /api/chat/media-download?storageRelPath=…`（需登录） |
| IM 大文件外链 | — | `GET /api/media/public-download?token=…`（HMAC 限时，**无需登录**） |

Web 端点击附件时，`openAttachmentWithSystemDefault` 对 `storageRelPath` 走带 cookie 的 `fetch` 下载。

IM 出站超过直传上限时，服务端签发 `public-download` 链接，以 Markdown 形式写入通道文本（见 [channel-integration.md](../developer/channel-integration.md)）。

## 桌面截图（Web）

- API：`POST /api/computer/manual-snapshot` → 响应体为 **JPEG 二进制流**（`Content-Type: image/jpeg`，`Cache-Control: no-store`），不再 base64 包装 JSON
- 格式：JPEG（默认质量 **68**）；服务端捕获后按预览用途压缩（长边 ≤1280px，单张 ≤100KB），避免 5s 轮询占用过多带宽
- UI：Web 模式下侧栏「查看桌面」按钮（`DesktopSnapshotButton.vue`）
- 预览打开时每 **5 秒**自动刷新一次截图；关闭预览后停止刷新
- 显示的是 **pointer-server 进程所在主机** 的桌面（云 ECS = 云桌面）
- 预览图叠加 **合成鼠标指针** 与 **输入焦点 I-beam**（与 Computer Agent 视觉 overlay 一致；Linux 上焦点坐标可能不可用）

## ALB / 就绪（平台）

见 [pointer-official/apps/api/README.md](../../pointer-official/apps/api/README.md) 与 [cloud-host-integration.md](../developer/cloud-host-integration.md)。
