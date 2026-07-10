# Web 附件与桌面截图

## 附件（Web 与桌面）

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
