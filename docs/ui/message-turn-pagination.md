# 消息按用户回合分页

右侧对话消息按**完整用户回合（turn）**分页加载，降低长会话首屏体积。LLM / `run_chat` 仍从 SQLite 全量读取 transcript，与 UI 窗口无关。

## 行为

| 场景 | 行为 |
|------|------|
| 打开会话 | 默认加载最近 **8** 个真实用户回合（含回合内 assistant / tool / sub-agent）；**不**因空壳首次水合拉全量 |
| SSE catch-up / cron 刷新 | `ensureMessagesLoaded({ force })` 会重新拉取，但仍是 **8 回合窗口**（`force` ≠ 全量） |
| 加载更早 | 向上滚到距顶约 300px **自动预取**（无顶栏按钮、无加载中态） |
| 没有更早 | 已到会话开头后，再往上拉出空白区域时才显示「没有更早的消息」；松手/滚回后收起 |
| 滚顶节流 | **严格串行**：上一页 IPC+渲染锚定未完成前不发下一页；自动预取每次需先滚离顶部再武装；忙时直接 skip，不并发、不 join |
| 滚动不跳 | 加载更早 / 内存裁剪后按**可见 turn id + 视口内偏移**重钉，不用 `scrollHeight` 差值（虚拟行估算高度会变） |
| 更早页收缩 | 加载更早后对历史行做与首屏相同的中断态归一化，避免卡住的 `streaming` 被当成进行中而不收缩 |
| 侧栏 FTS 定位 | `aroundMessageId` 拉取命中所在回合窗口，再滚动高亮 |
| 对话内 ⌘F | **仅搜索已加载消息**；未加载的更早内容不会命中 |
| 内存裁剪 | 按 user 消息 `viewedAt`：加载/发送即打戳，进视口续期；超过 **10 分钟**可裁（至少保留最近 24 个 user 回合）；滚回顶部从 SQLite 再取 |

## 内存裁剪与打戳

| 时机 | `viewedAt` | 作用 |
|------|------------|------|
| 加载进内存（hydrate / load-older / around） | 加载时刻 | 避免「无戳永久保留」 |
| 新发 / IM 注入 user 消息 | `createdAt` / 产生时刻 | 新消息进入可裁时钟 |
| 进入虚拟视口 | 刷新为当前时间 | 刚看过的回合 10 分钟内不裁 |

裁剪算法（`computeHistoryTrimCutByViewedAt`）：从最老 user 往新扫，停在第一个「无戳或未过期」的保留锚点；若**全部已打戳且都过期**，则只受「至少保留 N 个 user 回合」地板约束（可裁到地板）。SQLite 仍有全量，滚上去会重新加载。

## API

`GET /api/conversations/:id/messages`（Tauri：`load_conversation_messages_page`）

| 查询参数 | 含义 |
|----------|------|
| （无） | 兼容：返回全量 `ChatMessage[]` |
| `limitTurns` | 回合窗口大小；`0` = 全量（`MessagePage`） |
| `beforePosition` | 加载该 `position` **之前**的完整回合 |
| `aroundMessageId` | 以该消息所在回合为中心扩窗 |

有分页参数时响应为：

```json
{
  "messages": [],
  "hasMoreOlder": true,
  "hasMoreNewer": false,
  "oldestPosition": 12,
  "newestPosition": 40,
  "messageCount": 120
}
```

## 实现位置

- `crates/pointer-core/src/conversation_store/message_page.rs`
- `src/stores/chat.ts` — `ensureMessagesLoaded` / `loadOlderMessages` / `ensureMessagesAround`
- `src/components/chat/MessageList.vue` — 滚顶自动预取、turn-id 视口锚定、开头提示
- `src/lib/messageListScrollAnchor.ts` — 锚定 scrollTop 计算

## Schema note (`is_system_generated`)

Turn anchors use `messages.is_system_generated` (schema v21). Migration backfills
**user rows only** (synthetic / scoped), gated by `store_meta.is_system_generated_backfilled`,
inside one transaction. Do not UPDATE every message row — that bloated the WAL and
blocked desktop launch on large local DBs.

## 非目标

- 对话内搜索服务端 FTS
- 改 compression / transcript `begin()`
