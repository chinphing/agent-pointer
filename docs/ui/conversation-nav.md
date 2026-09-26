# 对话导航

会话主区**右缘短横条**（对标 Codex）：每条真实用户消息一根线，点击跳到该回合。不占文字栏，不提取 Markdown 标题。这是**回合大纲**，不是对话列表的滚动条；不要按滚动条去读它的疏密。

## 行为

| 场景 | 行为 |
|------|------|
| 打开会话 | `GET /conversations/:id/outline` 拉全量用户回合（只要预览） |
| 外观 | 贴在对话列右缘（外侧约 4px），整列按内容高度垂直居中（不撑满）。当前回合横条最醒目；**已加载窗口内**的其余横条略亮（约 0.55 透明度），窗口外更淡（约 0.22）。悬停鱼眼放大。超过约 30 条时列内可滚（**不画滚动条**）。仅当首/末横条被裁切时才显示顶/底箭头 |
| 滚轮 | 指针在导航上时只滚导航列，到头即停 |
| 悬停 | 左侧浮出该条用户消息预览。鱼眼最宽的一条对齐鼠标所在横条 |
| 里程碑 | 用户消息脚注（时间、复制旁）可标为里程碑，再点一次取消。标上后该条改成强调色菱形：比对应短横更宽，高度铺满这一格。只对会进导航的真实用户消息开放。标记写在消息行上，重写对话正文不会清掉 |
| 点击横条 / 预览 | 与侧栏搜索相同：`aroundMessageId` 水合窗口，再滚到该消息。预览可点，避免点穿到下方消息。若当前窗口还有更新内容（`hasMoreNewer`）或内存里已有越过游标的直播回合，且点的是**最后一根**横条，改为加载真正尾部，避免把 around 窗口里的最后一条当成全文结尾。around 若只拿到 scoped 空页，保持当前窗口，不换成欢迎页 |
| 贴底 | 对话已滚到真正底部时，当前横条为**已加载的最后一轮**（不是视口上方 18% 的标记）。around 窗口的底部不是全文结尾，此时仍显示「跳到最新」 |
| 点击箭头 | 平移导航列，露出被挡住的横条（不滚动对话列表） |
| 窄屏 | 同样贴右缘（不占工作区）；横条很窄，不另做浮层面板 |

条目与分页锚点一致：`role=user` 且 `is_system_generated=0`（截图注入、压缩摘要、`【环境反馈】` / `【输出长度】` 等重试胶不进导航）。预览约 36 字。

## API

桌面：`list_conversation_outline`。网页：`GET /api/conversations/:id/outline`。

```json
[{ "messageId": "…", "preview": "…", "milestone": true }]
```

`milestone` 只在标了里程碑时出现。桌面：`set_message_milestone`。网页：`POST /api/conversations/:id/messages/:messageId/milestone`，正文 `{ "milestone": true }`。

## 实现位置

- `crates/pointer-core/src/conversation_store/persist.rs`
- `src/components/chat/ConversationNav.vue`
- `src/components/chat/MessageList.vue` — 视口内当前回合（加深横条）
- `src/lib/conversationNav.ts`

相关：[message-turn-pagination.md](message-turn-pagination.md)。
