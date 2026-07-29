# 多媒体支持设计方案

> 按需理解：上传仅落盘；模型通过 API 清单感知附件；Agent 显式调用 `media_understand` 或 Office Skill。

---

## 1. 目标

- Composer 支持图片、文档、音频、视频附件（**视频** OSS 上限 **5 GB**；**>500 MB** 需用户确认后压缩；IM 入站视频同策略但 **>500 MB 自动压缩**；非视频 IM 媒体 **30 MB**）
- 主模型保持用户所选 agent 模型不变
- **上传不自动理解**：`apply_media_to_history` 仅落盘 + 写 `attachments`
- **模型上下文**：`make_openai_messages` 追加 Markdown 清单（`fileName` + `ref` + `localPath`）；用户附件用 `<!-- pointer-user-attachments -->`，助手 `MEDIA:` 交付附件用 `<!-- pointer-delivered-attachments -->`（均为 API-only，不写回 `content`）
- **按需理解**：`media_understand`（image/video/audio/pdf 扫描件回退）。
  何时调用 / 用户指定其他读取方式：以工具提示词 **When to call** 为准（此处不重复）。
  **`mode` 可选**（按后缀推断；视频转写显式 `audio`）。
- Office / PDF：**docx** / **xlsx** / **pptx** / **pdf** Skill + terminal（`localPath`）；PDF 阅读优先 skill，扫描件才 `media_understand`
- App（Tauri）与 Web 端行为一致

---

## 2. 架构

```mermaid
flowchart TD
    Composer[Composer] --> PayloadStore[attachmentPayloadStore]
    PayloadStore --> Send[sendChat messages + attachments]
    Send --> Apply[apply_media_to_history 仅落盘]
    Apply --> Attachments[attachments 元数据]
    Attachments --> UI[UserMessageBubble 预览]
    Attachments --> MakeOpenAI[make_openai_messages]
    MakeOpenAI --> Manifest[API Markdown 清单]
    Manifest --> Agent[主 Agent]
    Agent -->|意图不明| Clarify[追问]
    Agent -->|明确| MediaUnderstand[media_understand]
    Agent -->|Office| OfficeSkill[docx/xlsx/pptx Skill]

    subgraph display [Display]
        Persisted[Persisted attachments] --> Normalizer[messageNormalizer]
        Normalizer --> Bubble[UserMessageBubble]
    end
```

---

## 3. 数据模型

### 3.1 ChatMessage.attachments

| 字段 | 说明 |
|------|------|
| `id` | New saved attachments use a 12-character lowercase hexadecimal ID; historical rows may use UUID |
| `kind` | `image` / `document` / `audio` / `video` / `file` |
| `mimeType` | MIME |
| `fileName` | 原始文件名 |
| `sizeBytes` | 大小 |
| `storageRelPath` | New files are relative to `session-sandboxes/.../attachments/`; UI preview and path resolution use it |
| `contentBase64` | **仅 wire**，持久化前剥离 |
| `derivedText` | 可选缓存（`media_understand` 结果）；不写回 `msg.content` |

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
| 千问 DashScope | `wan2.7-image-pro` | `happyhorse-1.0-t2v`（有首帧图时自动切 `happyhorse-1.0-i2v`） |
| 豆包 Volcengine Ark | `doubao-seedream-5-0-lite-260128` | `doubao-seedance-2-0-fast-260128`（Seedance 2.0 极速/轻量） |

路由规则：**仅**使用设置中的 `mediaModelOverrides.imageGeneration` / `videoGeneration`（`providerId` + `model`）；未配置时若存在豆包 provider 则默认豆包，否则千问。工具参数 **`model` 已移除/忽略**，不由 AI 指定模型。

---

## 4. 后端模块

| 模块 | 路径 | 职责 |
|------|------|------|
| capabilities | `media/capabilities.rs` | `model_supports_vision`（千问=true、深度求索=false，按服务商固化） |
| store | `media/store.rs` | 落盘、读取、media ticket 路径 |
| apply | `media/apply.rs` | 仅落盘、清 wire base64 |
| manifest | `media/manifest.rs` | API-only Markdown 附件清单 |
| media_ref | `media/media_ref.rs` | `resolve_media_ref`（pointer-media / 绝对路径） |
| understand | `media/understand.rs` | 按需理解 LLM 调用（供 `media_understand`） |
| media_understand | `tools/media_understand.rs` | 宿主工具注册与 async dispatch |
| media_generation | `media_generation/` | `image_generate` / `video_generate` 工具：DashScope Wan/Qwen-Image、Volcengine Seedream/Seedance |
| media_generate | `tools/media_generate.rs` | 工具注册与 async dispatch |

挂载点：`session_inner::run_chat_inner`，在 `maybe_compress_history` **之前**调用 `apply_media_to_history`（传入 `run_id`，多媒体理解 token 写入 `token_usage_store`，独立 `agent_instance_id`）。

生成工具 async 路径：`agent_tool_pass.rs` → `dispatch_media_generate_async`；产出保存至 `generated-media/{conversation_id}/`，工具结果含 `MEDIA:<path>` 行。计费上报支持 `tokens` / `per-image` / `per-sec` 三种模式（见 `media_generation/billing.rs`）。

**`final_reply` 工具**：注册时使用 `ToolEntry::with_final_reply(true)`（默认 `false`）。当该工具**单独**调用且成功时，宿主不再发起后续 LLM 回合，而是将工具输出作为最终 assistant 消息交付（含 `MEDIA:` 附件内联展示）。当前启用：`image_generate`、`video_generate`。

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
| 单图 inline 上限 | 1 MB |
| 单图硬上限 | 6 MB |
| 文档文本提取上限 | 256 KiB |
| PDF 页图理解（扫描回退） | 10 页 / call；单页 JPEG ≤ **6 MB**（Pdfium 渲染 + `media_understand`） |
| 单视频 Composer 上限 | **100 MB** |
| Composer video | 允许（需本机 ffmpeg 方可理解） |

### 6.1 大文件与复杂附件场景

上传一律**完整落盘**；理解阶段有硬上限。清单含 `sizeBytes`，Agent 应结合 `goal` / `context` 做**分片、指定范围、或换工具**。

#### 超大 PDF

| 阶段 | 行为 |
|------|------|
| 默认范围 | 用户**未明确要求页码**时，仅处理 **第 1–10 页**；工具结果含 scope 说明 |
| 用户指定页码 | Agent 传 **`pageStart` / `pageEnd`**（1-based，含首尾）；未指定则不传 |
| 单次上限 | 每 call 最多 **10 页**；更多页码须**多次** `media_understand` |
| 文本型 PDF | **pdf** Skill 抽文本（Python/`terminal`） |
| 扫描/图片型 PDF | pdf Skill 判定无可用文本 → **`media_understand`**（后缀推断 `pdf`；也可显式 `mode=pdf`；Pdfium 逐页渲染 → 视觉模型） |

**Agent 策略**

- 用户说「第 45–60 页」→ `pageStart=45`, `pageEnd=60`（若 >10 页则拆成多次 call）。
- 用户只说「总结这份 PDF」→ 不传页码参数，默认 1–10 页；结果里告知用户范围。
- 全书摘要：说明默认仅前 10 页；可分批或让用户指定页码。

#### 超大视频

| 阶段 | 行为 |
|------|------|
| 上传 | Composer 默认 **OSS**（`remoteUrl`）；**>500 MB** 弹窗确认后压缩至 ≤500 MB 再上传；IM 入站**视频**同 OSS 路径，**>500 MB 自动压缩**；非视频 IM **≤ 30 MB** |
| 理解主路径 | DashScope **`video_url`** + **`fps`**，输入为 **`remoteUrl`**（HTTPS OSS URL） |
| 回退 | 无 `remoteUrl` 或原生 API 失败 → ffmpeg 抽 JPEG 帧 + vision |
| 工具结果 | scope 含时间窗、总时长、输入模式（native / ffmpeg fallback） |

**Agent 策略**

- 用户说「前 2 分钟」→ 在 **goal** 写明时间段；回退抽帧路径默认只处理首段，需多次 call 时在 **goal** 说明后续段落。
- 用户只说「总结这个视频」→ **goal** 概括需求即可；主路径送整段 `remoteUrl`。
- 全文转写 → 提音轨 + `mode=audio`。

#### 多 Sheet Excel（.xlsx）

走 **`xlsx` Skill** + `terminal` + **`localPath`**（与 `media_understand` 无关，见 Office 架构）。

#### 图片目录（多图）

| 阶段 | 行为 |
|------|------|
| ref | **`mode=image`** 时 **refs** 单元素可为本地目录路径 |
| 列举 | 仅**当前目录**（非递归）；png/jpg/jpeg/gif/webp/bmp/heic/heif；按文件名排序 |
| 默认 | 用户未指定范围 → **第 1–200 张** |
| 用户指定 | **`pageStart` / `pageEnd`**（1-based 序号，与 PDF 共用参数名） |
| 单次上限 | **200 张**；更多须多次 call |
| 工具结果 | scope 含本次序号范围、**目录内总张数**、拆分指引 |

---

| 类型 | 建议 |
|------|------|
| **大图片** | 自动缩放到 ≤ **6 MB** JPEG 再 vision |
| **长音频** | ASR 模型有上下文上限；超长录音在 `goal` 中说明「只要结论/某段」；必要时分段转写 |
| **pdf** | **pdf** Skill 优先（`terminal` + `localPath` 抽文本）；**扫描件**才 **`media_understand`**（`mode` 可省略，按后缀推断） |
| **docx / pptx** | **docx** / **pptx** Skill，不用 `media_understand` |
| **zip / 二进制** | 不支持内联；`skill_read` 或追问用户要提取什么 |
| **多附件** | 统一 **refs** 数组；图片可多个，音视频/PDF 仅单元素 |
| **超大附件已落盘但理解失败** | 工具结果会含截断/页数说明；向用户解释限制并给出替代（指定范围、拆文件、用 Skill） |

---

## 7. 分阶段

- **P0（已完成）**：图片 + 文档、vision 分支、Composer UI、imageModel 默认
- **P1（已完成）**：设置页 image/audio 理解模型、语音转写注入、audio bubble 播放
- **P2（已完成）**：IM 渠道 inbound 媒体、PDF/视频理解、ffmpeg Skill 引导安装
- **P2b'（已完成）**：扫描 PDF：`media_understand` Pdfium 逐页渲染 → 视觉模型（无 poppler CLI 依赖）
- **P2b''（已完成）**：内置 **pdf** Skill；附件阅读 **skill 优先**，扫描件回退 `media_understand`；脚本抽文本默认不 sort

---

## 8. 设置项

### 模式选择（用户可见）

通用 Agent、编程 Agent、多媒体理解与电脑操控均在 **设置 → 智能体 → 模式选择** 中配置运行模式；用户选模式，不直接选模型。

| 区域 | 模式作用 |
|------|----------|
| 通用 / 编程 Agent | 主会话 LLM 按所选模式解析模型 |
| 图片 / 语音 / 视频理解 | 非 vision 主模型或抽帧理解时按所选模式解析模型 |
| 电脑操控 | 新会话初始视觉模式（快速 / 标准 / 专家，对应 primary / intermediate / advanced） |

各模式对应的具体模型在 **调试模式** 下配置（`agentModeLlm` / `mediaModeLlm` / `computerTierLlm`）。

### 图片 / 视频生成（用户可选模型）

`image_generate` / `video_generate` 仍暴露模型下拉；选项来自各服务商 `models` 列表，并按 `modelConfigs` 中 **可生成图片 / 可生成视频** 过滤。

生成类模型已并入千问、豆包等服务商配置；每个模型可配置：

| 字段 | 含义 |
|------|------|
| `supportsVision` | 是否支持视觉理解 |
| `canGenerateImage` | 是否可生成图片 |
| `canGenerateVideo` | 是否可生成视频 |

**视觉理解**按服务商固化：千问下所有模型默认 `supportsVision: true`，深度求索下均为 `false`（可在 **设置 → 模型服务 → 模型定制** 按模型覆盖）。图片/视频**生成**能力仍按模型 id 关键字推断。

### 旧版 `mediaModelOverrides`

仍作为 legacy 回退（无模式配置时）；新安装默认走模式解析。

在 **设置 → 智能体 → 图片 / 视频生成** 中配置：

| 项 | 作用 | 默认 |
|----|------|------|
| 图片生成 | `image_generate` 工具 | wan2.7-image-pro 或 Seedream 5.0 |
| 视频生成 | `video_generate` 工具 | HappyHorse 1.0 或 Seedance 2.0 |

配置写入 `local_platform_settings.json`，重启后保留。

### 计费记录

生成调用写入 `token_usage_store`，独立 `agent_instance_id`（`media-image-generate` / `media-video-generate`）：

| 模式 | 来源 | 记录方式 |
|------|------|----------|
| ProviderTokens | DashScope 响应 `usage.total_tokens` | 原样上报 |
| PerImage | Seedream 等按张计费 | 合成 tokens（约 1 万张 = 1e8 tokens 量级映射） |
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
| Composer | 支持 video 上传（**100 MB**） | — |
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

Composer 视频在添加附件时**默认上传 OSS**（不走 Base64 传输）；上传完成前 Composer **禁止发送**。`attachments` 写入 `remoteUrl` / `ossObjectKey`，API 清单对视频注入 `remoteUrl`。超过 **500 MB** 不能直接上传：弹窗确认后 Pointer 压缩到 500 MB 以下再上传（帧率/分辨率可能降低）；取消则不添加附件。OSS PutObject 单次上限约 5 GB。

`media_understand` 理解视频时优先使用 `remoteUrl` 送入 DashScope `video_url`；失败或无 `remoteUrl` 时回退 ffmpeg 抽帧。

IM 入站**视频**大小策略与 Composer OSS 一致（上限 **5 GB**；**>500 MB** 自动压缩后上传）。非视频 IM 媒体仍为 **30 MB**。**入站视频**在 `apply_media_to_history` 中与 Composer 共用 OSS 上传（`remoteUrl`）；OSS 未配置或上传失败时仍用本地文件 + ffmpeg 回退。

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
…请 skill_read(dev-env-setup) → path references/ffmpeg.md …
```

| 场景 | 行为 |
|------|------|
| App 主对话 | general 助手加载 Skill，`terminal` 安装（需用户批准） |
| IM 入站 | 回复短文案 + 提示在 Pointer 客户端说「帮我安装 ffmpeg」 |

### 10.5a 不支持附件 / 处理失败：find-skills 引导

zip、Office（docx/xlsx/pptx）等 Composer 可上传但后端无法内联解析的类型，以及
图片/PDF/音频/视频理解失败时，`apply_media` 注入带标记的提示块：

```text
<!-- pointer-unsupported-attachment -->
<!-- pointer-media-processing-failed -->
…优先级：① 已启用 Skill（模型自判匹配）→ ② find-skills 按需搜索安装 → ③ 写代码最后手段
```

| 场景 | 行为 |
|------|------|
| App 主对话 | 先查「可用 Skills」；有匹配则直接 `skill_read`，**勿**重复 `npx skills find` |
| 无匹配 | 征得同意后 `find-skills` → 搜索安装 |
| 仍不可行 | `terminal` 一次性脚本或 `coder`（最后手段） |
| 工具审批 | `terminal` / `skill_import` 是否弹批准卡片由 **toolApprovalMode** 决定，无额外 UI |
| IM 入站 | 简短说明需在 Pointer 客户端继续（搜索/安装技能） |

### 10.5b 附件重试（无需重发文件）

首次处理失败后，附件字节已保存在用户 sandbox 的 `attachments/` 目录（`storageRelPath`）。用户无需重传：

| 触发 | 行为 |
|------|------|
| 用户说「重试上一条附件」/「重试上一条视频」 | 从 `storageRelPath` 重跑 `apply_media`，替换原消息中的失败注入块 |
| ffmpeg 从未就绪变为就绪 | 自动重试含 `<!-- pointer-media-deps -->` 的失败视频 |
| 已成功（`derivedText` 有值） | 不重试 |

范围：话术重试默认扫描最近 3 条带失败附件的用户消息；视频重试仅处理 `kind=video`。

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
| polish' | 设置页 ffmpeg 探测执行 `-version`；抽帧失败与未安装区分提示 | 已完成 |
| polish'' | 图片/语音/视频/扫描 PDF 理解 token 计入 `token_usage_store`（独立 instance_id） | 已完成 |
| polish''' | 媒体 `agent_instance_id` 改为 UUID v5（平台 `request_id` 按 `:` 分段，不可含冒号） | 已完成 |
| polish'''' | 不支持附件与媒体处理失败注入 `find-skills` 引导标记 | 已完成 |
| polish''''' | 附件重试：从 `storageRelPath` 重处理，话术触发 + ffmpeg 自动重试 | 已完成 |
