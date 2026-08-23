# 对话导航

会话主区**右缘短横条**（对标 Codex）：每条真实用户消息一根线，点击跳到该回合。不占文字栏，不提取 Markdown 标题。

## 行为

| 场景 | 行为 |
|------|------|
| 打开会话 | `GET /conversations/:id/outline` 拉全量用户回合（只要预览） |
| 外观 | 贴在对话列右缘（外侧约 4px），整列按内容高度垂直居中（不撑满）。初始横条约 5px 宽、1.5px 高；悬停鱼眼放大。超过约 30 条时列内可滚（**不画滚动条**）。仅当首/末横条被裁切时才显示顶/底箭头 |
| 滚轮 | 指针在导航上时只滚导航列，到头即停 |
| 悬停 | 左侧浮出该条用户消息预览。鱼眼最宽的一条对齐鼠标所在横条 |
| 点击横条 | 与侧栏搜索相同：`aroundMessageId` 水合窗口，再滚到该消息 |
| 贴底 | 对话已滚到真正底部时，当前横条为**已加载的最后一轮**（不是视口上方 18% 的标记） |
| 点击箭头 | 平移导航列，露出被挡住的横条（不滚动对话列表） |
| 窄屏 | 同样贴右缘（不占工作区）；横条很窄，不另做浮层面板 |

条目与分页锚点一致：`role=user` 且 `is_system_generated=0`（截图注入、压缩摘要、`【环境反馈】` / `【输出长度】` 等重试胶不进导航）。预览约 36 字。

## API

桌面：`list_conversation_outline`。网页：`GET /api/conversations/:id/outline`。

```json
[{ "messageId": "…", "preview": "…" }]
```

## 实现位置

- `crates/pointer-core/src/conversation_store/persist.rs`
- `src/components/chat/ConversationNav.vue`
- `src/components/chat/MessageList.vue` — 视口内当前回合（加深横条）
- `src/lib/conversationNav.ts`

相关：[message-turn-pagination.md](message-turn-pagination.md)。
