# 侧栏切换会话的选中反馈

点击超长会话时，若同帧内重建消息列表布局，浏览器来不及先绘制侧栏
`is-active`，会感觉「加载完才选中」。

## 约定

1. **pointerdown 乐观高亮**：侧栏行在 `pointerdown` 时立刻标为选中
   （`optimisticConversationId`），不必等 `click` → `openConversation` 跑完。
2. **`currentId` 尽早提交**：`selectConversation` 在 kickoff hydrate 后立刻写
   `currentId`，并同步切换草稿 / run state；驱逐空闲会话与任务板刷新放到
   `requestAnimationFrame`，避免堵住首帧。
3. **hydrate 仍先于 `currentId` 打 loading 标**：`ensureMessagesLoaded` 须在改
   `currentId` 前启动，保证 `isCurrentConversationHydrating` 在 MessageList
   watcher 同步触发时为 true（pending focus 定位竞态）。
4. **内存中的超长 transcript**：已 hydrate 且消息数 ≥ 80 时，主区先骨架一到两帧
   再挂 `MessageList`，让侧栏选中态先上屏。

## 相关代码

- `src/components/layout/AppShell.vue` — 乐观高亮
- `src/stores/chat.ts` — `selectConversation`
- `src/components/chat/ChatView.vue` — `deferHeavyTranscript`
