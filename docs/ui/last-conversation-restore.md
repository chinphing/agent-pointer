# 启动恢复上次会话

下次打开应用（桌面 / Web）时，主区应回到**上次选中的会话**，而不是侧栏分页的第一行。

## 约定

1. **本地记忆**：选中会话时把 id 写入 `localStorage` 键
   `pointer.chat.lastConversationId`（App 与 Web 共用同一前端逻辑）。
2. **启动选中**：`chat` store `init` 加载第一页 meta 后，优先恢复该 id：
   - 已在已加载 shells 中 → 直接选中；
   - 不在第一页 → `loadConversationMeta` 拉取 meta、注入 shell 再选中；
   - 不存在 / 越权（ListScope）→ 清除本地键，回退到当前列表最新一条。
3. **删除与登出**：删除该会话或平台账户退出登录时清除本地键（删除后若切到另一会话，会由选中逻辑重新写入）。
4. **跨入口**：Tauri 命令 `load_conversation_meta` 与 Web
   `GET /api/conversations/:id/meta` 均按侧栏 `ListScope` 过滤。

## 相关代码

- `src/lib/lastConversation.ts` — 读写本地键
- `src/stores/chat.ts` — `resolveBootConversationId` / `watch(currentId)`
- `crates/pointer-core/src/storage.rs` — `load_conversation_meta`
