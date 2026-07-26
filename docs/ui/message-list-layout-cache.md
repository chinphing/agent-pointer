# MessageList 布局缓存（已完成 turn 指纹）

长会话流式输出时，`messages` / tool / task board 会高频变更。虚拟列表只限制 DOM，
但 `flatten` + `buildConversationTurns` 若每次全量重跑仍是 O(n)。

## 约定

1. 按 user 消息将 transcript 切成 turn 段。
2. 每段算**结构指纹**（message id / role / status / 展示 kind / 可见 tool id+status /
   board `storeKey|version|status|isActive` / summary·delivery 标记）。
   **不含** content、reasoning、tool args/result 正文。
3. 与上一帧缓存比对：连续匹配且非 `active` 的前缀 turn 复用（rebind 到当前 message /
   board 引用）；**最后一轮始终重算**（流式落点）。
4. 压缩、中途注入、旧 turn 状态/看板变更会使指纹失配，从失配处起重算。
5. 切会话时清空缓存。

实现：`src/lib/messageListLayout.ts`，由 `MessageList.vue` 调用。
