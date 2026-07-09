# 侧边栏会话搜索 Snippet

侧边栏搜索命中会话后，列表第二行展示的是 **match-centered snippet**（命中关键词附近的上下文），不是会话 meta 里的 `preview`（最新用户消息摘要）。

## 行为约定

1. **匹配**：`messages_fts`（全文）+ `title` / `preview` 子串补充。
2. **展示文案**：从 `messages` 表取正文，按查询词做靠前截取（`text_util::match_centered_snippet`，命中靠近 snippet 开头，避免侧栏 CSS `truncate` 裁掉关键词）。
3. **按会话去重**：不要只用 bm25 第一条消息；若该条没有连续子串命中，再在同会话内 `LIKE` 查找真正包含查询词的消息。
4. **不要**依赖 SQLite FTS5 `snippet()` 作为 UI 预览。
## 相关代码

- `crates/pointer-core/src/conversation_store/search.rs` — `search_conversations_for_ui` / `collect_fts_hits`
- `crates/pointer-core/src/text_util.rs` — `match_centered_snippet`
- `src/components/layout/AppShell.vue` — 展示 `snippet`（无 snippet 时才回退 `preview`）

## 输入防抖与空态

搜索框有 300ms 防抖。`searchQuery` 一变，列表就切到 `searchResults`；若此时才把 `searchLoading` 设为 true（等请求发出），防抖窗口内会出现「有查询词 + 空结果 + 未加载」→ 闪一下「没有找到匹配的会话」。

约定：查询非空时**立刻** `searchLoading = true`，再启动防抖；空态仅在 `!searchLoading && !sidebarRows.length` 时展示。
