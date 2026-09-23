# 回合工作耗时（「工作 X m YY s」）

折叠 / 展开切换按钮旁展示该 user turn 的工作耗时。
展开后文案仍保留（与常规收缩交互一致）。
箭头在文案**之后**。收起时桌面悬停或键盘聚焦才显现；**展开后固定显示**（否则看不出已展开）。触控端始终显示。

默认前缀为「工作」（`工作 N m SS s` / `工作耗时未知`）。Standalone server 可通过 branding 覆盖进行中 / 结束后前缀，见 [web-branding-welcome-elapsed.md](web-branding-welcome-elapsed.md)。定制部署若要在进行中显示收起条，请打开助手设置「默认收缩执行过程」（server **不**强制该开关）。

耗时条贴在用户问题（或任务板）下面。下一条助手正文相对用户气泡的间距用 `mt-1.5`（6px），与工具行紧贴用户气泡时相同，不再用无耗时条时的 `mt-7`（28px）。折叠、展开都一样。

## 与「默认收缩执行过程」的关系

设置项：`collapseProcessByDefault`（助手设置「默认收缩执行过程」）。

| 设置 | 行为 |
|------|------|
| **关**（默认） | 与增加该配置**之前**一致：进行中回合不折叠（无耗时条）；回合结束后，最新一轮有隐藏过程时自动展开并显示耗时条；更早回合折叠显示耗时条。 |
| **开** | 进行中回合也可折叠并显示实时耗时；已完成轮次默认折叠。 |

实现要点：

- `buildConversationTurns(..., { collapseActiveTurns, omitDeliveryWhileActive })`：仅开启时对 `active` 回合计算 `hiddenCount`；进行中不挂最终 delivery，避免中间叙述当「最终输出」
- 收缩投影（`messageListLayout.projectCollapsedTurn`）：保留的 delivery 标 `contentOnly`，去掉 `trailingToolGroups` 与非交互工具行；`ask_user` / `pending_approval` **以及仍要露出子任务框的 `run_subagent` 宿主行**仍可见（进行中，或宿主已结束但子 trace 仍在跑 / 还有未完成询问）。子 Agent 内的询问挂在宿主下方，不能把整段 SubAgentFrame 藏掉；**任务板（含进行中）始终保留**（进度 chrome，不是可隐藏过程）
- `MessageList` 传入 `collapseActiveTurns: collapseProcessByDefault`
- 未开启时 `shouldAutoExpandTurn` 仍要求 `state !== 'active'`

收缩态可见内容：**用户问题 + 任务板 + 回合结束后的最后一次 assistant content**，以及贴在下一条真实用户问题前的前缀压缩芯片。切在 tool/assistant 上的前缀芯片与 in-run 摘要一样，随工具过程藏进「工作」。thoughts / **已结束的**子 Agent 过程仅在展开后显示；**进行中的子 Agent**（含其收缩态露出的 `ask_user`）仍显示。

加载更早（`loadOlderMessages`）写入内存后必须跑 `normalizeInterruptedAssistantStatuses`：历史里若仍带 `streaming`/`pending`，在「默认收缩」关闭时会被当成 active 而不折叠。

切回仍在生成的会话时：先 `reconcileRunStateForConversation`（从 in-flight 工具 / streaming 行恢复 `generating`），再决定是否 `normalizeInterrupted`。顺序反了会把进行中工具打成终态，首屏出现「工作」耗时条且缺少当前工具行，直到下一次 tool_call 才刷新。

## 展开 / 收起状态（跨会话切换与刷新）

`MessageList` 只持久化**用户显式覆盖**：`expandedTurnIds` / `manuallyCollapsedTurnIds`（以及变更摘要的展开/收起覆盖）。隐式行为（`shouldAutoExpandTurn`、变更摘要条数启发式）不入库，刷新后按规则重算。

| 场景 | 行为 |
|------|------|
| 切会话再切回 | 恢复该会话的覆盖态 |
| 刷新 / 重开应用 | 同左（`localStorage` 按登录用户分桶） |
| 切换账号 | 内存清空；各账号磁盘桶互不共享；再登录恢复自己的桶 |
| 登出 | 仅清内存并回到 `anon` 桶，**不**删各账号已存偏好 |
| 删除会话 | 清当前用户桶内该会话条目 |
| 「默认收缩执行过程」开关 | 不改覆盖集；只改变 `turnIsExpanded` 是否叠加自动展开 |

实现：`src/lib/turnExpandState.ts`（`pointer.chat.turn-expand.v1::{userId}`；session 的 `userId` 来自 `PlatformSessionView`）。旧无用户 key 会一次性迁入当前用户桶。`MessageList` 在 toggle / 定位展开 / 切 `currentId` / unmount 时写入。

注意：硬杀进程且从未触发写入时仍可能丢最后一次点击；正常点击已即时落盘。

## 计时口径

| 来源 | 含义 |
|------|------|
| **主路径** | `recordTurnStart`（`dispatchChatTurn`）→ `recordTurnDone`（`Done` / 中断结算） |
| **回退** | 同 turn 内首条 user 与末条消息的 `createdAt` 差（无本地计时记录时） |

展示优先用主路径（`resolveTurnElapsedMs`）。开启默认收缩且回合进行中时，用 `activeTurnStartedAt` + 1s ticker 实时刷新。

## 出站队列 /「立即发送」

- 队列项入队时的 `createdAt` 只表示**入队时刻**（队列面板用）。
- **真正写入会话**时必须用**派发时刻** `Date.now()`，不得沿用入队时间。
  否则回退口径会把排队等待算进「工作耗时」。
- 「立即发送」会 `interruptActiveTurn({ cancelBackgroundJobs: false })`：先 `recordTurnDone` 结算被打断的回合，再 drain 下一轮；后台 job 不杀。
  `recordTurnStart` 在 turnId 变化时也会结算上一轮（双保险）。
- 被取消 run 的迟到 `Error` / `Done` 都不得关掉新回合的计时 / `generating`（`isStaleStreamAfterInterrupt` 先识别，`consumeStaleDoneAfterInterrupt` 只在 Done 上消费）。迟到 `Error` 若先结算，会把下一轮写成 **工作 0 m 00 s**。
- 已写入的不足 1 秒记录，若同轮消息 `createdAt` 跨度更长，展示回退到消息时间戳（修复历史 0s）。
- 回退窗口遇到子 Agent stub / 环境反馈 user 行不截断（它们不是新的用户回合）。

### Composer 快捷键（对齐 Cursor）

| 按键 | 行为 |
|------|------|
| **Enter**（有草稿） | 空闲则发送；生成中则入队 |
| **Enter**（空草稿 + 队列非空） | 立即发送队首（软取消：只停同步，后台继续） |
| **⌘/Ctrl+Enter** | 软取消当前回合并立即发送：有草稿则先入队再 force-send；无草稿则 force-send 队首 |
| **停止按钮** | 硬取消：lead + 本会话全部后台 |

实现：`Composer.vue`（`onKeydown` / `stopAndSendNow`）→ `forceSendOutbound`。

## 实现位置

- `src/lib/turnElapsed.ts`
- `src/lib/turnExpandState.ts`（按会话缓存展开 / 收起，localStorage）
- `src/lib/conversationTurns.ts`（`collapseActiveTurns`）
- `src/lib/messageListLayout.ts`
- `src/stores/chat.ts`（`dispatchChatTurn` / `drainOutboundQueue` / `interruptActiveTurn`）
- `src/components/chat/MessageList.vue`（`turnElapsedLabel`）
- `src/stores/chat/streamHandlers/sessionHandlers.ts`（`handleDone`）
