# Web 附件与桌面截图

## 附件（Web 与桌面）

| 能力 | 桌面 (Tauri) | Web (pointer-server) |
|------|--------------|----------------------|
| 图片/音频预览 | `previewChatMedia` | 同左（JSON base64 API） |
| 视频预览 | `convertFileSrc` | `GET /api/chat/media-stream?storageRelPath=…` |
| 文件打开/下载 | OS 默认应用 | `GET /api/chat/media-download?storageRelPath=…` |

Web 端点击附件时，`openAttachmentWithSystemDefault` 对 `storageRelPath` 触发浏览器下载。

## 桌面截图（Web）

- API：`POST /api/computer/manual-snapshot` → `{ imageBase64, imageMime, caption }`
- UI：Web 模式下侧栏「查看桌面」按钮（`DesktopSnapshotButton.vue`）
- 显示的是 **pointer-server 进程所在主机** 的桌面（云 ECS = 云桌面）

## ALB / 就绪（平台）

见 [pointer-official/apps/api/README.md](../../pointer-official/apps/api/README.md) 与 [cloud-host-integration.md](cloud-host-integration.md)。
