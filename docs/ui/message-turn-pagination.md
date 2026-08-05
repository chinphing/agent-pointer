# 消息按用户回合分页

右侧对话消息按**完整用户回合（turn）**分页加载，降低长会话首屏体积。LLM / `run_chat` 仍从 SQLite 全量读取 transcript，与 UI 窗口无关。

## 行为

| 场景 | 行为 |
|------|------|
| 打开会话 | 默认加载最近 **8** 个真实用户回合（含回合内 assistant / tool / sub-agent） |
| 加载更早 | 列表顶「加载更早消息」，或向上滚到距顶约 300px 自动预取 |
| 滚顶节流 | 在途锁 + 落地后 200ms 冷却；仅向上滚触发 |
| 侧栏 FTS 定位 | `aroundMessageId` 拉取命中所在回合窗口，再滚动高亮 |
| 对话内 ⌘F | **仅搜索已加载消息**；未加载的更早内容不会命中 |

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
- `src/components/chat/MessageList.vue` — 顶栏按钮、滚顶预取、prepend 锚定

## 非目标

- 对话内搜索服务端 FTS
- 视口外远端页卸载
- 改 compression / transcript `begin()`
