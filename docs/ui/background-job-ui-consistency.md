# 后台 Job 与子 Agent UI 状态一致性

后台 `run_subagent` / `terminal` 的界面曾反复出现：已结束仍显示「后台执行中」、随后又变成「已取消」、标题与统计行拆开。根因是 **多套状态各写各的**，没有单一真相源。本文约定统一模型与修复方向。

## 现象（对照）

| 现象 | 常见触发 |
|------|----------|
| 宿主行一直「后台执行中」，正文已写「全部完成」 | `ToolCallStatus(success)` 未落到内存 UI（流丢失 / 父轮已 Done），宿主仍 `running` |
| 过一会变成「已取消」 | 占用快照 `backgroundJobs=0` → `finalizeOrphanBackgroundHosts` 把仍 `running` 的宿主标成 `failed` + `error=interrupted`；`ToolCallRow` 把 interrupted 显示成「已取消」 |
| 标题（C1…）与「代码探索 · 搜索 N 次」拆开 | `SubAgentFrame` 未挂在宿主 `after-tool`（`tool_run` 路径未接线，或 `parentToolCallId` 对不上 → orphan 堆在底部） |

## 单一真相源

对**每一条**后台宿主工具行（`run_subagent` + `background`，或 `terminal` + `blockUntilMs`），UI 状态只认下面优先级：

1. **宿主 `ToolCall.status` + `result` 句柄**（权威，须与 JobSupervisor / 落盘一致）
2. 句柄 JSON：`{ jobId, status, kind }` 中的 `status`（`running` / `completed` / `failed` / `cancelled`）
3. **禁止**用「占用计数 = 0」单独推断「用户取消」；占用为 0 只表示 JobSupervisor 里没有 running，**不能**默认写成 interrupted

前端识别 `run_subagent` 后台宿主时须与后端一致：`self` / `explore` / `coder` **省略** `background` 也视为后台；句柄 JSON（`kind=subagent`）亦视为后台。回合 `Done` 后 `finalizeStuckToolCalls` **不得**因句柄 JSON 已写入而把仍 `running` 的宿主标成 `success`。

对应文案：

| 宿主 status | 句柄 status | 文案 |
|-------------|-------------|------|
| `running` | `running`（或缺句柄） | 后台执行中 |
| `success` | `completed` | 已完成 |
| `failed` | `failed` | 失败 |
| `failed` | `cancelled` 或 error 匹配 cancel/interrupted | 已取消 |
| `running` 但句柄已是终态 | （对账滞后） | **先按句柄纠正宿主**，再显示终态文案 |

嵌套 `SubAgentFrame` / `agent_step.status` 是**过程视图**，不代替宿主终态。trace `completed` 而宿主仍 `running` = 不同步，必须修宿主。

## 写入路径（必须成对）

```
spawn background
  → ToolCallStatus(running) + handle{status:running}   // 「后台执行中」
  → agent_step(running, parentToolCallId)              // 统计挂到该宿主下

job 终态（任意：正常结束 / 失败 / 取消）
  → ToolCallStatus(success|failed) + handle{终态}      // 宿主文案
  → agent_step(completed|failed|cancelled)             // 过程帧终态
  → background_jobs(runningCount)                      // 占用 / 侧栏转圈
  → 落盘同一 message 的 toolCalls[i]
```

`job.await` 只把 **content** 交回父模型，**不**负责改宿主文案；宿主必须在 job 终态时已由上面路径更新。await 成功而宿主仍「后台执行中」= bug。

父轮 `Done` 之后后台仍可跑完：`StreamTx` / 落盘仍须送达；前端不得在 Done 后忽略该会话的 `ToolCallStatus`。

空闲 push 会在同一会话插入一条用户消息并再开一轮 lead。消息带 **`uiBindings.hostKind=idle_job_push`** + **`bubbleText`**：「后台任务已完成。」；完整终稿在 `content` 里进 lead 上下文。UI 按 `bubbleText` 画气泡。`InjectedUserMessage` 须进当前会话列表（与 cron 注入相同）。宿主行终态仍由上面的 job 终态路径写入，不要等 push 才改「后台执行中」。

## 占用对账（重启 / 弱网）

| 步骤 | 正确行为 | 错误行为（曾发生） |
|------|----------|-------------------|
| 快照 `backgroundJobs` 某会话 = 0 | 清空占用计数 | — |
| 内存里仍有 `running` 后台宿主 | 先按句柄收敛；句柄仍是 `running` 则 **拉落盘再覆盖宿主**；落盘也是 running 且监督器已空才标 interrupted | 一律 `failed` + `interrupted` → 误显示「已取消」 |
| 落盘已是 success | hydrate 覆盖内存，显示「已完成」 | 用 orphan 逻辑盖成取消 |

`background_jobs(0)` / 占用快照该会话为 0 时：若内存仍有 live 宿主，`reconcileBackgroundHostsWhenOccupancyEmpty` 只补宿主 `status`/`result`（不整表替换会话），再跑 `finalizeOrphanBackgroundHosts`。强制水合若走 `mergeHydratedMessages`，必须用 **DB 页** 再 apply 一遍（live overlay 会保住旧的 `running` 工具行）。

`finalizeOrphanBackgroundHosts` 只处理「监督器已空且句柄仍声称 running」的僵尸行，并打 info 日志。打开会话时先 `rehydrate` + `repairBackgroundHostsFromChildOutcomes`（子 trace 已终态则提升宿主），再 apply 落盘、再 orphan，避免脏库被标成「已取消」。`rehydrate` **不得**把仍是 `running` 的 live trace 改成「失败」；终态只认 `agent_step`。对话导航跳到中间再滚回底部时，force-tail 必须带上宿主，不能只留下 scoped 子行。

父会话短列表 `sync_messages_ordered` **不得**用内存里仍是 `running` 的后台宿主盖掉库里已终态的同一 `toolCall`（`conversation_store/background_host_merge.rs`）。`complete_background_host_tool` 同时改宿主 `toolCalls` 和对应 `role:tool` 句柄行。

## 布局一致性

1. **嵌套键**：`AgentTrace.parentToolCallId` === 宿主 `toolCall.id`。后台 running 的第一条 `agent_step` 就必须带上（与前台 owned-wave 相同）。
2. **渲染点**：凡画出 `run_subagent` 宿主行的地方，必须提供 `#after-tool` → `SubAgentFrame`（含 `MessageList` 的 `tool_run` 段，不能只用裸 `ToolMessageSegment`）。
3. **orphan**：仅兼容无 `parentToolCallId` 的旧数据；打开时按 `taskId` / `traceId` 前缀 / 剩余宿主顺序补绑定，新会话不应依赖底部 orphan 堆。
4. **布局指纹**：`messageStructureFingerprint` 须包含 agentTrace 的 id/status/`parentToolCallId`，否则 tool_only→带 trace 的结构切换会被 completed-turn 缓存冻住。
5. **落盘覆盖**：后台宿主一旦写成 success/failed，后续 lead working-set sync 不得把它打回 running。

## 验收

1. 开 4 个 `self` / `explore` 子 Agent（省略 `background` 或 `true`）→ 各宿主「后台执行中」，其下紧贴对应「代码探索 · 搜索…」。
2. job 跑完（或 `job.await` 成功）→ 宿主立刻「已完成」，不再「后台执行中」；耗时与前台一样显示整数秒（≥1s）。
3. 进程重启后打开同会话：已完成的仍「已完成」，不得批量变「已取消」。
4. 用户真点 **停止**（取消全部后台）：才批量显示「已取消」。
5. 行上「结束任务」只取消这一条；「结束等待」/ 立即发送只停同步，其它后台仍「后台执行中」。
6. **结束等待**后：主助手行不得显示「已停止生成」；`job.await` 显示「已结束等待」；后台宿主行仍显示「后台执行中」（子帧收缩摘要只写工具次数，不重复「后台执行中」）。
7. 会话有后台占用时，Composer 上方**始终**显示后台任务条（父回合仍在 generating / `job.await` 时也显示）；侧栏会话仍转圈（`isConversationBusy`）。停止按钮仍可「停 lead + 全部后台」；条上可展开并单独结束某一条。

## 与 Cursor / Codex 的对照

| 产品 | 做法 | Pointer 对应 |
|------|------|--------------|
| **Cursor** | 侧栏 Agents 列表 + 状态色点（运行/等待/完成）；当前会话 busy 时侧栏转圈；云 Agent 完成推送通知 | 侧栏 `Loader2`（generating 或 backgroundJobs）；离开会话完成时 solid dot |
| **Codex** | Composer 上方状态条；子 Agent 面板展示活动 | Composer 上方后台占用条（**有占用即显示**，含父回合仍在 generating / `job.await` 时）；`SubAgentFrame` 收缩行 + 宿主「后台执行中」 |
| **共性** | 占用态与主线程生成态分离展示，避免「看起来已经停了」 | 结束等待 = soft cancel，不占「已停止生成」；后台行置顶 + Composer 条 |

## 相关实现

- 后端：`run_subagent_delegation.rs`（`complete_background_host_tool` / `emit_background_jobs`）、`conversation_store/background_host_merge.rs`、`AppState::cancel_with_options` / `cancel_background_jobs`
- 前端：`toolHandlers.ts`、`helpers.ts`（`applyPersistedBackgroundHostOutcomes` / `repairBackgroundHostsFromChildOutcomes` / `finalizeOrphanBackgroundHosts`）、`subAgentMessages.ts`（rehydrate 绑定 `parentToolCallId`）、`chat.ts`（`interruptActiveTurn` / `endWaitKeepBackground` / `cancelBackgroundJob`）、`ToolCallRow.vue`、`AssistantModelMessage.vue`、`MessageList.vue`、`messageListLayout.ts`
- 设计背景：`docs/design/async-subagent-and-terminal.md`、`docs/developer/chat-stream-resync.md`
