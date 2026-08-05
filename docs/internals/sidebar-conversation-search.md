# 侧边栏会话搜索 Snippet

侧边栏搜索命中会话后，列表第二行展示的是 **match-centered snippet**（命中关键词附近的上下文），不是会话 meta 里的 `preview`（最新用户消息摘要）。

## 行为约定

1. **匹配**：`messages_fts`（全文）+ `title` / `preview` 子串补充。
2. **展示文案**：从 `messages` 表取正文，按查询词做靠前截取（`text_util::match_centered_snippet`，命中靠近 snippet 开头，避免侧栏 CSS `truncate` 裁掉关键词）。
3. **按会话去重**：不要只用 bm25 第一条消息；若该条没有连续子串命中，再在同会话内 `LIKE` 查找真正包含查询词的消息。
4. **不要**依赖 SQLite FTS5 `snippet()` 作为 UI 预览。
5. **点击定位**：正文命中时结果带 `messageId`；点击后打开会话，用 `aroundMessageId` 拉取命中所在回合窗口（见 [message-turn-pagination.md](../ui/message-turn-pagination.md)），再滚动到该消息（必要时扩大虚拟渲染窗口），短暂高亮。仅标题/preview 命中时无 `messageId`，行为与原先一致（只打开会话并加载尾部回合）。
6. **分页外命中**：搜索覆盖全库，侧栏 meta 仅加载首屏分页。点击不在内存列表中的会话时，须用搜索命中的 title / `messageCount` **注入临时 shell** 再 hydrate；否则 `current` 为空，主区会误显示欢迎空白页。
7. **消息窗口**：会话消息默认只 hydrate 最近若干用户回合；定位旧命中必须走 around 窗口，不能假设全量已在 `conv.messages` 中。

## 相关代码

- `crates/pointer-core/src/conversation_store/search.rs` — `search_conversations_for_ui` / `collect_fts_hits`
- `crates/pointer-core/src/text_util.rs` — `match_centered_snippet`
- `src/components/layout/AppShell.vue` — 展示 `snippet`（无 snippet 时才回退 `preview`）；点击传入 `focusMessageId`
- `src/stores/chat.ts` — `pendingFocusMessage` / `selectConversation(..., { focusMessageId })` / `ensureMessagesAround`
- `src/components/chat/MessageList.vue` — 消费 pending focus：缺消息时 around 加载、扩窗、`scrollIntoView`、高亮
- [message-turn-pagination.md](../ui/message-turn-pagination.md) — 回合分页与 around 窗口

## 输入防抖与空态

搜索框有 300ms 防抖。`searchQuery` 一变，列表就切到 `searchResults`；若此时才把 `searchLoading` 设为 true（等请求发出），防抖窗口内会出现「有查询词 + 空结果 + 未加载」→ 闪一下「没有找到匹配的会话」。

约定：查询非空时**立刻** `searchLoading = true`，再启动防抖；空态仅在 `!searchLoading && !sidebarRows.length` 时展示。
