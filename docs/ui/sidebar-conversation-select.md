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
5. **切回进行中会话**：hydrate 先 reconcile 恢复 `generating`，再 normalize
   interrupted；合并 DB 页时保留 in-flight 工具行。否则首屏会误显示「工作」耗时、
   缺少当前工具行（见 [turn-elapsed.md](turn-elapsed.md)）。

## 行尾操作（置顶 / 删除）

Hover 时标题从左侧淡入操作区，不要用整段 `backdrop-blur` 盖住图标。

- 渐变只在**左图标左侧**约 20px，接到与当前行相同的底色（侧栏 / hover / 选中）。
- 两个图标坐在**不透明底**上，缝里不能透出标题字。
- 图标本身保持清晰。桌面与网页同一套样式（`AppShell` `.sidebar-row-actions`）。
- **会话搜索结果**不展示置顶 / 删除，避免挡住「N 处」。见 [sidebar-conversation-search.md](../internals/sidebar-conversation-search.md)。

启动时恢复上次选中会话见 [last-conversation-restore.md](last-conversation-restore.md)。

## 相关代码

- `src/stores/chat.ts` — `selectConversation`
- `src/components/chat/ChatView.vue` — 主区一帧延迟
- `src/components/layout/AppShell.vue` — 侧栏 `currentId` 高亮
