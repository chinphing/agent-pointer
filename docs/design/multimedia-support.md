# 多媒体支持设计方案

> 参考 OpenClaw 双管线架构（输入 wire / 显示 normalize）。主会话模型不强制切换，按能力分支 inline 或 media-understanding 理解后注入文本。

---

## 1. 目标

- Composer 支持图片、文档、音频、视频附件（视频单文件上限 30 MB，与 IM 入站一致）
- 主模型保持用户所选 agent 模型不变
- Vision 主模型：图片 inline 发给 LLM
- 非 Vision 主模型：独立 `imageModel` 理解图片，描述文本注入 user message
- App（Tauri）与 Web 端行为一致

---

## 2. 架构

```mermaid
flowchart TD
    Composer[Composer] --> PayloadStore[attachmentPayloadStore]
    PayloadStore --> Send[sendChat messages + attachments]
    Send --> Parse[parse_message_attachments]
    Parse --> Branch{Primary vision?}
    Branch -->|Yes| Inline[images_base64 inline]
    Branch -->|No| ImageModel[imageModel describe]
    ImageModel --> Inject[inject text into content]
    Inline --> PrimaryLLM[Primary model unchanged]
    Inject --> PrimaryLLM
    PrimaryLLM --> MakeOpenAI[make_openai_messages]

    subgraph display [Display]
        Optimistic[Optimistic preview] --> Normalizer[messageNormalizer]
        Persisted[Persisted attachments] --> Normalizer
        Normalizer --> Bubble[UserMessageBubble]
    end
```

---

## 3. 数据模型

### 3.1 ChatMessage.attachments

| 字段 | 说明 |
|------|------|
| `id` | UUID |
| `kind` | `image` / `document` / `audio` / `video` / `file` |
| `mimeType` | MIME |
| `fileName` | 原始文件名 |
| `sizeBytes` | 大小 |
| `storageRelPath` | 相对 `conversation-media/{convId}/` |
| `contentBase64` | **仅 wire**，持久化前剥离 |
| `derivedText` | 文档提取或 imageModel 描述（可选缓存） |

Computer Agent 截图仍用 `imagesBase64` inject，与用户附件 `attachments` 分离。

### 3.2 mediaModelOverrides

```typescript
mediaModelOverrides: {
  image?: { providerId: string; model: string }  // 默认 qwen + qwen3.5-plus
  audio?: { providerId: string; model: string }
  video?: { providerId: string; model: string }
  imageGeneration?: { providerId: string; model: string }  // image_generate 工具
  videoGeneration?: { providerId: string; model: string }  // video_generate 工具
}
```

生成工具默认模型（未配置 override 时）：

| 服务商 | 图片 | 视频 |
|--------|------|------|
| 千问 DashScope | `wan2.7-image-pro` | `wan2.7-t2v` |
| 豆包 Volcengine Ark | `doubao-seedream-5-0-lite-260128` | `doubao-seedance-1-5-pro-251215` |

路由规则：优先 `mediaModelOverrides.imageGeneration` / `videoGeneration` 的 `providerId`；否则若配置了豆包 provider 则走豆包，否则走千问。工具参数 `model` 可单次覆盖。

---

## 4. 后端模块

| 模块 | 路径 | 职责 |
|------|------|------|
| capabilities | `media/capabilities.rs` | `model_supports_vision` 启发式 |
| store | `media/store.rs` | 落盘、读取、media ticket 路径 |
| apply | `media/apply.rs` | 编排理解、写 `images_base64` / 注入 text |
| understand | `media/understand.rs` | imageModel 单次 vision 描述 |
| media_generation | `media_generation/` | `image_generate` / `video_generate` 工具：DashScope Wan/Qwen-Image、Volcengine Seedream/Seedance |
| media_generate | `tools/media_generate.rs` | 工具注册与 async dispatch |

挂载点：`session_inner::run_chat_inner`，在 `maybe_compress_history` **之前**调用 `apply_media_to_history`。

生成工具 async 路径：`agent_tool_pass.rs` → `dispatch_media_generate_async`；产出保存至 `generated-media/{conversation_id}/`，工具结果含 `MEDIA:<path>` 行。

---

## 5. 前端模块

| 模块 | 路径 | 职责 |
|------|------|------|
| attachmentSupport | `src/lib/attachmentSupport.ts` | MIME accept、大小限制 |
| attachmentPayloadStore | `src/lib/attachmentPayloadStore.ts` | base64/object URL 不进 Pinia |
| messageNormalizer | `src/lib/messageNormalizer.ts` | 附件渲染模型 |
| AttachmentChip | `components/chat/AttachmentChip.vue` | Composer 预览条 |
| UserMessageBubble | 扩展 | 图片/文档卡片 |

---

## 6. 限制

| 项 | 值 |
|----|-----|
| 单图 inline 上限 | 2 MB |
| 单图硬上限 | 6 MB |
| 文档文本提取上限 | 256 KiB |
| 单视频 Composer 上限 | 30 MB |
| Composer video | 允许（需本机 ffmpeg 方可理解） |

---

## 7. 分阶段

- **P0（已完成）**：图片 + 文档、vision 分支、Composer UI、imageModel 默认
- **P1（已完成）**：设置页 image/audio 理解模型、语音转写注入、audio bubble 播放
- **P2（已完成）**：IM 渠道 inbound 媒体、PDF/视频理解、ffmpeg Skill 引导安装

---

## 8. 设置项

在 **设置 → 智能体 → 多媒体理解模型** 中配置：

| 项 | 作用 | 默认 |
|----|------|------|
| 图片理解 | 非 vision 主模型时描述图片 | qwen3.5-plus |
| 语音转写 | 音频附件 ASR（需模型支持 input_audio） | 同图片模型 |
| 视频理解 | IM 视频抽帧后多图理解（需 ffmpeg） | 同图片模型 |

在 **设置 → 智能体 → 图片 / 视频生成** 中配置：

| 项 | 作用 | 默认 |
|----|------|------|
| 图片生成 | `image_generate` 工具 | wan2.7-image-pro 或 Seedream 5.0 |
| 视频生成 | `video_generate` 工具 | wan2.7-t2v 或 Seedance 1.5 Pro |

配置写入 `local_platform_settings.json`，重启后保留。

### 计费记录

生成调用写入 `token_usage_store`，独立 `agent_instance_id`（`media-image-generate` / `media-video-generate`）：

| 模式 | 来源 | 记录方式 |
|------|------|----------|
| ProviderTokens | DashScope 响应 `usage.total_tokens` | 原样上报 |
| PerImage | Seedream 等按张计费 | 合成 tokens 映射 |
| PerVideoSecond | Seedance 1.x 按秒 | 合成 tokens（约 2 万 tokens/秒） |
| PerVideoGenerationToken | Seedance 2.0（API 未全面开放） | 预留，模型 key 后缀 `@video-tokens` |

模型名在用量库中带后缀 `@tokens` / `@per-image` / `@per-sec` 以区分计费维度。

---

## 9. 参考

- OpenClaw：`chat-attachments.ts`、`media-understanding/runner.ts`、`attachment-payload-store.ts`
- Pointer 现有：`images_base64`、`make_openai_messages`、`sub_agent_provider`

---

## 10. P2 详细设计

### 10.1 目标与边界

| 范围 | 做 | 不做 |
|------|-----|------|
| 入站 | 飞书 / 企微 / 钉钉 / **微信个人号** WS 图片、文件、语音、视频 | — |
| Composer | 支持 video 上传（30 MB） | — |
| ffmpeg | 系统安装，**不打包**进安装包 | 内置 sidecar |
| 出站 IM | 仍只发文本 | 回复图片/文件 |

主会话模型不切换；渠道下载后写入 `ChatMessage.attachments`，复用 `apply_media_to_history`。

### 10.2 架构

```mermaid
flowchart TD
    FeishuParse[Feishu parse message_type] --> InboundRef[InboundMediaRef]
    InboundRef --> Download[feishu/media download]
    Download --> MediaAtt[MediaAttachment contentBase64]
    MediaAtt --> Dispatch[dispatch.handle_inbound]
    Dispatch --> Apply[apply_media_to_history]
    Apply --> RunChat[run_chat]
```

### 10.3 数据模型

**InboundMessage** 扩展：

| 字段 | 说明 |
|------|------|
| `attachments` | `InboundMediaRef[]`，解析阶段仅含渠道 key |

**InboundMediaRef**（wire，不落会话持久化）：

| 字段 | 说明 |
|------|------|
| `kind` | `image` / `document` / `audio` / `video` / `file` |
| `feishuImageKey` / `feishuFileKey` | 飞书下载 key |
| `feishuResourceType` | `image` / `file` / `media` |

dispatch 下载后转为 `MediaAttachment`，与 Composer 一致。

**Dedup**：`message_id + sorted(attachment keys)`，避免同 id 多附件被误杀。

### 10.4 飞书（P2a）

| message_type | 解析 | 下载 API |
|--------------|------|----------|
| `text` | content.text | — |
| `image` | image_key | `GET /im/v1/images/{key}` |
| `file` / `audio` | file_key | `GET .../messages/{id}/resources/{key}?type=file` |
| `media` / `video` | file_key | 同上；502 时 fallback `type=media` |
| `post` | 富文本 + 内嵌 img/media | 逐项 resource 下载 |

单文件上限：**30 MB**（与 OpenClaw 默认一致）。

### 10.4a 微信个人号（iLink Bot）

| item `type` | 解析 | 下载 |
|-------------|------|------|
| `1` TEXT | `text_item.text` | — |
| `2` IMAGE | `image_item.media` + 可选 `aeskey` | CDN AES-128-ECB |
| `3` VOICE | `voice_item.text`（微信 ASR，P0）+ `media` | CDN 解密 + SILK→WAV（P2） |
| `4` FILE | `file_item.media` + `file_name` | CDN 解密 |
| `5` VIDEO | `video_item.media` | CDN 解密 |

CDN：`GET https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=...`

### 10.5 ffmpeg：Skill 引导安装（零打包体积）

未检测到 `ffmpeg` + `ffprobe` 时，`apply_media` 注入带标记的提示块：

```text
<!-- pointer-media-deps -->
…请 skill_load_instructions(dev-env-setup) → references/ffmpeg.md …
```

| 场景 | 行为 |
|------|------|
| App 主对话 | general 助手加载 Skill，`terminal` 安装（需用户批准） |
| IM 入站 | 回复短文案 + 提示在 Pointer 客户端说「帮我安装 ffmpeg」 |

### 10.6 子阶段

| 阶段 | 内容 | 状态 |
|------|------|------|
| P2a | 飞书 inbound + dispatch + dedup | 已完成 |
| P2a' | 企微 WS image/file/mixed | 已完成 |
| P2a'' | 钉钉 inbound 媒体 | 已完成 |
| P2a''' | 微信 iLink inbound 媒体（CDN+SILK） | 已完成 |
| P2b | PDF 文本提取 | 已完成 |
| P2c | ffmpeg 抽帧 + videoModel | 已完成 |
| polish | 设置页 ffmpeg 检测、video 气泡 | 已完成 |
