# 消息按用户回合分页

右侧对话消息按**完整用户回合（turn）**分页加载，降低长会话首屏体积。LLM / `run_chat` 仍从 SQLite 全量读取 transcript，与 UI 窗口无关。

## 绘制不变式

Store 里可以同时有「当前 SQLite 窗口」和「越过游标的直播行」（force-tail 叠加 / 流式）。**画出来的列表只能是连续窗口**：`messagesInCurrentPageWindow`。空洞窗口里不得把高 `position` 的生成行接到中间历史上，否则虚拟列表会看起来顺序错乱。

| 层 | 内容 |
|----|------|
| Store | around 窗口 ∪ 可选的 disconnected live（给 force-tail / 流式用） |
| 绘制 | 仅 `position ≤ newestPosition` 的窗口行；`hasMoreNewer` 时丢掉直播溢出 |
| 跟随 | 仅 `!hasMoreNewer`。空洞底部不是全文结尾，即使内存里已有直播行 |
| 预取更新 | 仅看 `hasMoreNewer`。不要因为末行是直播就停掉，否则中间空洞填不上 |
| 发送 | 若仍在空洞，先 `ensureMessagesLoaded({ force: true })` 换成尾部，再追加 user 行 |

跳到最新 / 切回无定位 / 点导航最后一根横条：加载真正尾部，而不是把空洞底部当成结尾。

## 行为

| 场景 | 行为 |
|------|------|
| 打开会话 | 默认加载最近 **8** 个真实用户回合（含回合内 assistant / tool / sub-agent）；**不**因空壳首次水合拉全量 |
| SSE catch-up / cron 刷新 | `ensureMessagesLoaded({ force })` 会重新拉取，但仍是 **8 回合窗口**（`force` ≠ 全量） |
| 加载更早 | 向上滚到距顶约 300px **自动预取**；**已在顶部**时继续上滑 / 下拉同样请求下一页（此时 `scrollTop` 无法再减小，不能只靠 scroll 事件）。无顶栏按钮、无加载中态 |
| 加载更新 | 搜索定位到中间窗口后，**向下**滚到距底约 300px **自动预取**下一页更新回合；**已在底部**时继续下滑 / 上滑同样请求下一页（此时距底为 0，`scrollTop` 无法再增大）。只接受比当前**分页游标**更新的回合；更早的页直接丢弃并关掉 `hasMoreNewer`，**不会**把顶部内容接到列表里。Store 里越过游标的直播行**不画进**空洞列表，也不参与「是否更早页」的丢弃判断。只要 `hasMoreNewer`，预取继续，把中间页填上。**不**一次拉到真正的尾部 |
| 滚动到底部 | 若当前窗口 `hasMoreNewer`（或内存里已有越过游标的直播回合），先 `ensureMessagesLoaded({ force })` 换成尾部 8 回合，再贴底。around 窗口上的 `toBottom` **不**宣称跟随、**不**隐藏跳转按钮（空洞底部不是全文结尾）。**任务执行中** force 尾部只保留进行中的回合，不把 around 中间窗口拼进尾部。直播回合的起点必须是**真实用户任务**，不能停在子 Agent 的 `Begin. Your assigned task` stub 上，否则会丢掉宿主行，滚回底部后把还在跑的子 Agent 重水合成「失败」。切走再回来（无定位）会换尾部；有搜索 pending focus 时仍由 locate 接管，不冲掉 around 窗口 |
| 再次打开（无定位） | 清除该会话残留的 pending focus；若仍停在 around 窗口（`hasMoreNewer`），强制加载尾部。切走再回来、清空搜索后点同一行，都会回到最新对话。替换窗口后丢弃尚未完成的 `loadNewer` / `loadOlder`，避免把中间页接到尾部上造成顺序错乱 |
| 发送 | 当前窗口还有更新内容时，先 force 尾部再追加 user 消息，避免新发言画在空洞窗口底 |
| 没有更早 | 仅当 `hasMoreOlder === false` 且已在顶部时，再往上拉出空白才显示「没有更早的消息」；分页基线缺失时先自愈，不把「拉不动」当成已经到头 |
| 滚顶节流 | **严格串行**：上一页 IPC+渲染锚定未完成前不发下一页；**自动**预取每次需先滚离顶部再武装（防 restore 短差连载）；**用户**在顶部继续上滑/下拉不受该武装限制；忙时直接 skip，不并发、不 join |
| 滚动不跳 | 加载更早 / 内存裁剪后按**可见 turn id + 视口内偏移**重钉，不用 `scrollHeight` 差值（虚拟行估算高度会变） |
| 更早页收缩 | 加载更早后对历史行做与首屏相同的中断态归一化，避免卡住的 `streaming` 被当成进行中而不收缩 |
| 侧栏 FTS 定位 | `aroundMessageId` 拉取命中所在回合窗口，再滚动高亮 |
| 对话导航 | 全量用户消息目录（`outline` API），点击走同一套 around 定位；**不**拉 Markdown 标题。点最后一根横条且当前是空洞窗口时，加载真正尾部，不 around-merge |
| 对话内 ⌘F | **仅搜索已加载消息**；未加载的更早内容不会命中 |
| 内存裁剪 | 按 user 消息 `viewedAt`：加载/发送即打戳，进视口续期；超过 **10 分钟**可裁（至少保留最近 8 个 user 回合）；滚回顶部从 SQLite 再取 |

## 内存裁剪与打戳

| 时机 | `viewedAt` | 作用 |
|------|------------|------|
| 加载进内存（hydrate / load-older / load-newer / around） | 加载时刻 | 避免「无戳永久保留」 |
| 新发 / IM 注入 user 消息 | `createdAt` / 产生时刻 | 新消息进入可裁时钟 |
| 进入虚拟视口 | 刷新为当前时间 | 刚看过的回合 10 分钟内不裁 |

裁剪算法（`computeHistoryTrimCutByViewedAt`）：从最老 user 往新扫，停在第一个「无戳或未过期」的保留锚点；若**全部已打戳且都过期**，则只受「至少保留 N 个 user 回合」地板约束（可裁到地板）。SQLite 仍有全量，滚上去会重新加载。

## API

`GET /api/conversations/:id/messages`（Tauri：`load_conversation_messages_page`）

| 查询参数 | 含义 |
|----------|------|
| （无） | 兼容：返回全量 `ChatMessage[]` |
| `limitTurns` | 回合窗口大小；`0` = 全量（`MessagePage`） |
| `beforePosition` | 加载该 `position` **之前**的完整回合 |
| `afterPosition` | 加载该 `position` **之后**的完整回合（用于 around 窗口向尾部翻页） |
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
- `src/stores/chat.ts` — `ensureMessagesLoaded` / `loadOlderMessages` / `loadNewerMessages` / `ensureMessagesAround`；发送前 force 尾部
- `src/stores/chat/helpers.ts` — `messagesInCurrentPageWindow`（绘制窗口）/ `hasDisconnectedLiveTail`
- `src/components/chat/MessageList.vue` — 只虚拟化窗口行；滚顶/滚底自动预取、顶部上滑/下拉请求更早、**底部下滑请求更新**、跳转最新、turn-id 视口锚定、开头提示
- `src/lib/messageListOlderPrefetch.ts` — 顶部预取 vs「没有更早」提示的判定
- `src/lib/messageListScrollAnchor.ts` — 锚定 scrollTop 计算

## Schema note (`is_system_generated`)

Turn anchors use `messages.is_system_generated` (schema v21). Migration backfills
**user rows only** (synthetic / scoped / provider retry glue such as `【环境反馈】`),
gated by `store_meta.is_system_generated_backfilled_v2`,
inside one transaction. Do not UPDATE every message row — that bloated the WAL and
blocked desktop launch on large local DBs.

## 非目标

- 对话内搜索服务端 FTS
- 导航提取 Markdown 标题
- 改 compression / transcript `begin()`
