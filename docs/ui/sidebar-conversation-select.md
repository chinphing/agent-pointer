# 侧栏切换会话的选中反馈

点击超长会话时，若同帧内重建消息列表布局，会拖住侧栏 `is-active` 的首帧绘制。

## 约定

1. **`click` 先提交选中**：`selectConversation` 同步写入 `currentId`（侧栏高亮跟
   `currentId`），不做 `pointerdown` 乐观态。
2. **界面加载异步**：hydrate / 草稿 / 任务板放到 `queueMicrotask` /
   `requestAnimationFrame`，不堵高亮。
3. **主区让一帧**：`ChatView` 在 `currentId` 变化后先骨架一帧再挂 `MessageList`，
   避免同帧布局挡住侧栏绘制。
4. **hydrate 与 focus**：未 hydrate 且 `messageCount > 0` 时
   `isCurrentConversationHydrating` 为 true，pending focus 仍会等加载完成。

启动时恢复上次选中会话见 [last-conversation-restore.md](last-conversation-restore.md)。

## 相关代码

- `src/stores/chat.ts` — `selectConversation`
- `src/components/chat/ChatView.vue` — 主区一帧延迟
- `src/components/layout/AppShell.vue` — 侧栏 `currentId` 高亮
