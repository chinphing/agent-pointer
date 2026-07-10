# Composer 视频：阿里云 OSS 上传

用户添加视频附件时直传 OSS，记录 `remoteUrl`，API 清单注入该 URL；上传完成前禁止发送消息。`media_understand` 理解时以 **`remoteUrl`（HTTPS）** 送入 DashScope 原生 `video_url`。

**IM / 渠道入站视频**（飞书、企微、钉钉、微信等）在 `apply_media_to_history` 中走同一套 `upload_composer_video_bytes`：下载落盘后若 OSS 已配置则上传并写入 `remoteUrl` / `ossObjectKey`；失败或未配置时保留本地副本（ffmpeg 回退）。**视频**大小策略与 Composer 一致：单文件上限 **5 GB**（PutObject）；**>500 MB** 时 IM 入站**自动压缩**至 ≤500 MB 再上传（无弹窗，因无用户确认入口）。非视频 IM 媒体仍为 **30 MB**。

## 流程

1. 用户选择视频 → 前端调用 `uploadComposerVideoToOss`（Tauri 事件进度 / Web XHR 进度）
2. OSS PutObject（`public-read` 或 Bucket 策略允许 DashScope 拉取）→ 返回 HTTPS URL
3. `MediaAttachment.remoteUrl` / `ossObjectKey` 随消息发送；`apply_media_to_history` 不落本地字节
4. API 清单（`format_user_attachments_api_manifest`）对 `kind=video` 输出 `remoteUrl`

## 大小策略

| 阈值 | 行为 |
|------|------|
| ≤ 500 MB | 正常上传 |
| > 500 MB | 弹窗确认；同意后 Pointer 用 ffmpeg 压缩到 500 MB 以下再上传（帧率/分辨率可能降低）；取消不添加附件 |
| ≤ 5 GB | OSS PutObject 硬上限 |

## 相关入口

| 入口 | API |
|------|-----|
| Tauri 路径 | `upload_composer_video_to_oss` |
| Tauri 字节 | `upload_composer_video_bytes_to_oss` |
| Web | `POST /api/chat/upload-video-oss` |

对象前缀默认 `pointer-media-attachments/`。对象 Key 含 **`YYYYMM`** 月份目录，便于按月经生命周期规则清理：

`{keyPrefix}{YYYYMM}/{attachmentId}/{fileName}`

示例：`pointer-media-attachments/202606/3204bd07-…/像素蛋糕完整示例-0513.mp4`

## 官方 API 依据

| 操作 | 文档 |
|------|------|
| PutObject（单次 ≤ 5 GB） | [PutObject](https://help.aliyun.com/zh/oss/developer-reference/putobject) |

实现使用 [ossify](https://crates.io/crates/ossify)（OSS V4 签名）。

## 配置

### 平台下发（生产默认）

OSS 凭据由 **Pointer 官网管理后台**（「OSS」页）配置，仅保存 `endpoint`、`accessKeyId`、`accessKeySecret`。客户端登录或刷新 `llm-credentials` 时从服务端拉取，**仅驻内存**，与 API Key 相同，**不写入** `user_settings.json`，客户端无 OSS 设置界面。

其余字段使用默认值：

| 字段 | 默认值 |
|------|--------|
| `enabled` | `true`（凭据齐全时） |
| `bucket` | `pointer-app-media` |
| `region` | 从 `endpoint` 解析（如 `oss-cn-hangzhou` → `cn-hangzhou`），否则 `cn-hangzhou` |
| `keyPrefix` | `pointer-media-attachments/` |
| `presignExpiresSec` | `604800`（7 天，OSS 预签名 URL 上限） |
| `deleteAfterUse` | `true` |

### 本地开发 / 环境变量（可选）

开发调试可用环境变量覆盖，无需官网配置：

| 变量 | 说明 |
|------|------|
| `POINTER_MEDIA_OSS_ENABLED` / `OSS_MEDIA_UPLOAD_ENABLED` | `true` / `1` 启用 |
| `OSS_BUCKET` / `POINTER_OSS_BUCKET` | Bucket |
| `OSS_REGION` / `POINTER_OSS_REGION` | Region |
| `OSS_ENDPOINT` / `POINTER_OSS_ENDPOINT` | 可选 Endpoint |
| `OSS_ACCESS_KEY_ID` / `ALIBABA_CLOUD_ACCESS_KEY_ID` | AccessKey ID |
| `OSS_ACCESS_KEY_SECRET` / `ALIBABA_CLOUD_ACCESS_KEY_SECRET` | AccessKey Secret |
| `OSS_KEY_PREFIX` / `POINTER_OSS_KEY_PREFIX` | 对象前缀 |

## 跨平台与跨入口

- **macOS / Windows / Linux**：宿主内 `ossify` 直传 OSS。
- **Tauri / Web**：OSS 凭据经平台登录注入内存；上传逻辑在 `pointer-core` 统一执行。

## 安全与运维

- Bucket 建议配置策略，允许 DashScope 通过 HTTPS 拉取对象。
- 对象路径含 attachment id，避免覆盖与猜测。

## 相关代码

- `crates/pointer-core/src/media/oss.rs`（`upload_composer_video_*`）
- `src/lib/videoOssUpload.ts`、`src/components/chat/Composer.vue`
- `src-tauri/src/commands.rs`、`server/src/main.rs`（`upload-video-oss`）
- `crates/pointer-core/src/media/manifest.rs`（`remoteUrl` 清单）
- `crates/pointer-core/src/tools/media_understand/dispatch.rs`
- `crates/pointer-core/src/models/settings.rs`（`MediaOssConfig`）
