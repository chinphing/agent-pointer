# Cron IM deliver mirror（默认连贯）设计

## 目标

Cron / Run `deliver` 推到 IM 后，把同一正文 **mirror** 进对应 IM 桌面会话 transcript，
用户在通道里接着回时，模型能看到刚发过的内容（对齐 Hermes continuable /
`append_deliveries_to_session`，Pointer **默认开启**）。

## 行为

1. `ImDeliverHook` 在某目标 `send_outbound_explicit` **成功后**：
2. 按 `session_user_id`（= `recipient_id` / open_id）+ `{channel}:{account}:` 前缀
   查找最近活跃的 **DM** IM 桌面会话（飞书入站用 chat_id、home 用 open_id，
   不能只从 outbound `conversation_key` 拼 id）
3. `upsert` 一条 **assistant** 消息：`【定时投递】\n{可见正文}`
4. 广播 `InjectedAssistantMessage`（侧栏即时可见）
5. `touch_im_interaction(base)` 刷新空闲计时

无匹配会话时：跳过并打 info（用户须曾私聊过 bot）。

## 范围

| 目标 | Mirror |
|------|--------|
| Home / 显式 **DM** | 是（默认） |
| **群** | MVP 跳过（peer 依赖入站 sender，不稳定） |
| `[SILENT]` / 投递失败 | 不 mirror |

`im_send` 本期不改（可后做）。

## 非目标

- 引用回复注入 `[Replying to:]`
- 群会话 mirror
- 可关开关 UI（后续可加 per-job `mirrorSession=false`）
