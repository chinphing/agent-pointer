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
| 图片预览 | `previewChatMedia` / 本地路径 | 同左（JSON base64 API） |
| 视频 / 音频预览 | 本地 `convertFileSrc` 内联播放 | **不预览**，仅显示文件名 + 「下载」 |
| 文档 / 其他文件 | OS 默认应用打开 | `GET /api/chat/media-download` 或 `media-ref-download`（需登录） |
| IM 大文件外链 | — | `GET /api/media/public-download?token=…`（HMAC 限时，**无需登录**） |

Web 端点击附件下载时，走浏览器原生导航（`<a href>` + session cookie），由
`Content-Disposition: attachment` 立刻弹出下载栏，响应体服务端流式读盘，
**不要**再 `fetch` 整文件进 Blob 后再触发下载（大文件会长时间无反馈，且会撞
上通用请求 12s 超时）。可选查询参数 `fileName` 用于保留原始显示名（含中文，
`filename*`）。

Web 端**上传**走 `POST /api/chat/save-attachment`（**multipart/form-data**，字段
`conversationId` / `attachmentId` / `fileName` / `file`），与视频 OSS 上传一样用
XHR 以便显示进度。通用 JSON API 仍为 12s 超时；附件上传超时 **120s**。

进度约定：

- XHR `upload.onprogress` 到 **100%** 只表示**浏览器已发出**全部字节；服务端落盘 /
  压缩 / OSS PutObject / 签发 URL 仍在响应返回之前。进度条与文案**保留 100%**。
- 芯片在 `uploadProgress >= 100` 且尚未 `done` 时显示「处理中 100%」（不要写成
  「上传中 100%」，以免像已传完却卡住）。
- `uploadState === 'done'` 后显示「已上传」。
- 桌面视频 OSS（Rust）：PutObject 阶段 ≤99%，presign 成功后再报 100%。

交互约定：

1. 选文件后立刻用 `URL.createObjectURL` 出芯片缩略图（不必等读盘/上传）。
2. 添加时即开始上传（大图可先客户端压缩），芯片显示进度；发送时只带
   `storageRelPath`，不再重复传文件/base64。
3. 多文件并行添加与上传；未完成或失败时禁用发送。
4. 上传 / 发送遇网络或短暂服务端错误时自动重试最多 **3** 次（间隔约 0.8s、1.6s）；
   登录失效、余额不足、缺文件、**用户取消**等不重试。芯片上会短暂显示「重试中 n/3…」。
5. 芯片可手动**取消**进行中的上传（Web 端 abort XHR；桌面 invoke 无法中断底层传输，
   但会忽略结果并标为「上传已取消」），失败或取消后可点**重传**；移除（×）也会 abort。

桌面端仍通过 Tauri invoke 落盘（本地 base64/路径），同样在添加时上传并显示状态。

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
