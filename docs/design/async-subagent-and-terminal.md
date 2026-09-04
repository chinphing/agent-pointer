# 异步子 Agent 与后台终端

> 实现进度：已落地 JobSupervisor（job 表 + **按会话 FIFO 工人池**）、`run_subagent.background`（self/explore）、`terminal.blockUntilMs`、`job` list/status/await/cancel。前台 wave / 串行 coder·computer 与后台共用 `acquire_root`（嵌套 `acquire_nested`）。**空闲合并 push 已落地**。**`self` / `explore` 省略 `background` 默认后台**；`terminal` 与 `coder` / `computer` 默认仍为前台 join。**`job.await` 只在单个 job 终态时叫醒父模型**（内部工具过程不醒）。
>
> **终端：不传 `blockUntilMs` = 仍 join。`self` / `explore`：不传 `background` = 后台（空闲 push 已落地后）。`coder` / `computer` 始终前台。**
>
> 对照来源（2026-08）：
> [Cursor Subagents](https://cursor.com/docs/subagents.md)、
> [OpenAI Multi-agent](https://developers.openai.com/api/docs/guides/tools-multi-agent)、
> [Codex Subagents](https://developers.openai.com/codex/subagents)、
> Codex issues [#15723](https://github.com/openai/codex/issues/15723) / [#32188](https://github.com/openai/codex/issues/32188)。

## 问题

今天 `run_subagent` 和 Agent `terminal` 都在父会话**同一轮工具 pass 里阻塞**：

- 父 LLM 拿不到 tool result 就不能开下一轮。
- `session:{conversation}` 车道 `max=1`，子任务不结束，用户下一句会排在「待执行」。
- UI 能看到流式输出，父模型只能在 join 之后看到 `content`。
- 用户点停止会 cancel 整轮；没有「只停这一条后台、主会话继续」的句柄。

同轮 `self` / `explore` 并行 wave **不是**后台：子任务之间并发，父会话仍等 wave join。

右侧 Workspace PTY 是用户调试会话，**不要**拿来给 Agent 后台命令用。

## 对照：Cursor 与 Codex

两边都把「并行 fan-out」和「父线程不阻塞」拆开。Pointer 现状只做了前者。

| 维度 | Cursor | Codex / OpenAI Multi-agent | Pointer 现状 |
|------|--------|----------------------------|--------------|
| 前台委派 | `Task` 默认 Foreground：父工具卡住，结果当 tool result 回来 | 也可让父 `wait_agent` 卡住等邮箱 | `run_subagent` **只有这一种** |
| 后台委派 | `run_in_background: true`，或定义里 `is_background`；立刻返回，子任务自己跑 | `spawn_agent` 开线程；父不必立刻 wait | 无 |
| 父如何拿结果 | 完成后 **notify** 父（后台）；或 Foreground 内联返回 | 显式 `wait_agent`（邮箱有更新才醒）；`list_agents` 拉树 | 只能等 join |
| 空闲时子任务结束 | 父保持可交互，稍后通知 | 默认 `trigger_turn=false`：**不自动续轮**（#15723） | 不适用（父一直占着 lane） |
| 并行 | 同一条 assistant 消息里多个 `Task` | 多个 `spawn_agent` + 可选 wait | owned-wave：`self`/`explore` 并发，但仍 join |
| 续跑 | `resume` + agent id，上下文保留 | `followup_task` / `send_message` | 仅 `taskId` 续同一逻辑任务（仍阻塞） |
| 停一条 | `interrupt`（对正在跑的 async） | `interrupt_agent`（不停掉上下文） | `job.cancel` / 行上「结束任务」按 id；Composer 停止 = 全取消；立即发送 /「结束等待」= 软取消 |
| 槽位 | 内部并发上限 | `max_concurrent_subagents`；CLI 还有 `close_agent` 才让出槽 | `maxParallelSubAgents` 只限 wave |
| 隔离 | 默认同 checkout；可 worktree / `/in-cloud` VM | 同工具、同模型；树路径 `/root/...` | 同工作区；scoped 消息隔离上下文 |
| 过程可见 | UI 线程 + `~/.cursor/subagents/` 落盘 | 主线程里可点开子线程 | `SubAgentFrame` 流式，父模型看不到过程 |

### Cursor 值得学

1. **两档就够：Foreground / Background。** 不要一上来做 Codex 那套六动作（`spawn` / `send_message` / `followup_task` / `wait` / `interrupt` / `list`）。Pointer 已有 `run_subagent`，后台用一个 `background` 布尔即可。
2. **后台时父会话必须还能说话。** Cursor `/multitask` 相对旧 `Task` 的本质变化就是：子任务进后台，父立刻回到可交互。Pointer 的 `session:{conversation} max=1` 不解开，用户下一句仍会排队。
3. **过程给 UI，摘要给父模型。** 已对齐 Explore；后台时继续走现有 `agent_step` / `SubAgentFrame`，不要改成让父模型读落盘日志。
4. **定义级默认后台（后做）。** Cursor 自定义 agent 的 `is_background`；插件计划里已提到 `.cursor/agents` 该字段。P0 先工具参数，P2 再映射 frontmatter。
5. **可 resume。** 后台跑完不应丢掉 child scoped history。Pointer 已有 scoped 消息，P2 用现有 `taskId` + instance id 续跑即可。

### Codex 值得学

1. **后台启动后必须有显式 wait / list / cancel。** 只告诉模型「结束本轮等通知」在非交互或「必须跑完再答」时会丢结果。Codex 用 `wait_agent` + `list_agents`；Pointer 收成一个 `job` 工具。
2. **wait 要写清契约。** Codex `wait_agent` 等的是**调用者邮箱里任意一条更新**（适合滑动窗口），不是 wait-all；V1 醒了之后会把**已经终态的兄弟连同 `last_agent_message` 一并返回**。Pointer：`any` = 至少一条终态就醒，并 drain 本会话所有已完成未认领的正文；`all` = 这组齐了再返回。按 `jobIds` 过滤，超时后 job **继续跑**。
3. **空闲唤醒要 opt-in，且合并。** Codex 默认不续轮，社区普遍要 `on_exit: wake`。Pointer P2：会话空闲时 **一轮**内部 trigger，多条完成合并进同一条 user/tool 消息；取消 / 停会话 / shutdown **不**唤醒。
4. **完成投递要有认领。** 同一条完成不能既被 `await` 消费、又再 push 一轮（Codex `write_stdin` 与 watcher 双通道问题）。
5. **终端后台与子 Agent 同一套 job 表。** Codex unified exec 的 yield / 后台 session / `ExecCommandEnd` 与子 Agent 完成是同一类「父已闲、child 还在」。Pointer 用 `terminal.blockUntilMs` 对齐 yield，结果进同一个 `job`。

### 明确不抄

| 不抄 | 原因 |
|------|------|
| Codex 六动作拆工具 | 模型编排成本高；Pointer 已有 `run_subagent` |
| `wait_agent` 邮箱语义 | 嵌套路由含糊；改成按 jobId wait |
| 完成后必须 `close_agent` 才放槽 | Superpowers 已踩坑；终态且父已读结果后自动放槽 |
| Cursor 让父去读 `~/.cursor/subagents/` | 已有流式 trace；文件轮询是退路不是主路径 |
| 默认 worktree / 云 VM | 与现网同工作区不一致；隔离放到 P3 |
| 后台默认 `coder` / `computer` | 写冲突与桌面权限；P0 只 `self` / `explore` 与只读终端 |

## 目标

1. 父模型可以 **spawn 后立刻拿到 `jobId`**，同一轮或之后再 `await` / `cancel` / `status`。
2. 后台任务 **不占** `session:{conversation}`，用户可以继续说话。
3. 任务结束时主会话能感知：同轮用 pull（`job.await`）；会话空闲时可选 **一轮**合并 push。
4. 停止要杀得掉：Unix 进程组 + 按 `jobId` 收割；会话停止默认取消该会话全部后台 job。
5. App / Web 共用 `pointer-core`；macOS / Windows / Linux 都能杀进程树。

## 推荐契约

### `run_subagent.background`

| 值 | 行为 | 对齐 |
|----|------|------|
| 省略 / `true`（仅 **`self`** / **`explore`**） | 立刻 `{ jobId, status: "running", kind: "subagent" }`；child 在 JobSupervisor 里跑。这次调用的 tool result **保持句柄**，结束后只把 handle 的 status 改成 completed/failed/cancelled，不把工人 Markdown 写回 `run_subagent`。终稿走 `job.await` / 空闲 push | Cursor Background 默认档（Pointer：空闲 push 已落地后可默认） |
| `false` | 阻塞到结束，tool result 带 `content` | Cursor Foreground |
| **`coder`** / **`computer`** | 始终前台 join；传 `background: true` 工具失败 | 写冲突 / 桌面权限 |

P0 起仅 `self` / `explore` 可后台。其它 agentId 传 `background: true` 时工具失败并说明须前台 join。

### `terminal.blockUntilMs`

| 值 | 行为 | 对齐 |
|----|------|------|
| 省略 | 等到命令结束（现网） | Codex 前台 exec |
| `0` | 立刻返回 `{ jobId, status: "running", kind: "terminal" }` | Codex 后台 session |
| `N>0` | 最多等 N ms，未结束则 detach 成 job；结束则本轮返回完整 stdout JSON | Codex yield / 超时后转后台 |

P0 不含 elevated、需交互 stdin 的命令。Workspace 用户终端仍独立。

句柄与 `job` 回包必须带 **`kind`**：`subagent` 是工人，`terminal` 是 shell。不要把终端 job 当成子 Agent。`job.await` 里 subagent 的 `content` 是 Markdown，terminal 的 `content` 是命令 JSON。UI 上终端行仍是终端行（实时输出、「结束命令」）；detach 期间显示「后台执行中」，不要把句柄 JSON 当控制台输出。命令真正结束后，把完整终端 JSON 写回原来的 `terminal` 行（与子 Agent 不同：工人终稿永不写回 `run_subagent`）。

### 滑动窗口：谁完成谁让槽

并发上限是 **同时 running 的槽数**，不是「这一波要等齐」。先结束的 child **立刻退出并放槽**，不必等最慢的兄弟。

| 场景 | 现网 owned-wave | 后台滑动窗口 |
|------|-----------------|--------------|
| 同一则消息里已列出 10 个任务、上限 4 | 信号量会补位（4 跑完再接下一个），但父 LLM 仍卡到 10 个全 join | JobSupervisor 同样补位；父已拿到 10 个 `jobId`，可去干别的 |
| 下一任务要看**已完成者的结果**才知道 | 做不到：父被整波 join 堵住，空槽闲着 | `job.await(any)` 先完成的先把 `content` 交回；父立刻 `run_subagent` 补一个。其它仍在跑的继续占槽 |

这就是 Cursor 后台 `Task` 和 Codex `wait` 不要 wait-all 真正省掉的浪费：慢任务不必拖住快任务腾出来的槽。

`mode=any` 等到至少一条未认领终态，然后把**此刻所有已完成未认领的**都放进 `jobs[]`（带完整 `content`）并认领——对齐 Codex V1 `wait_agent` 的 drain-ready（先醒，再 `now_or_never` 收齐已终态的兄弟）。`running[]` 仍是 id。`any` 与 `all` 的差别只是要不要等最慢的，不是「正文给几个」。LLM 往返期间新完成的无法打断生成（P2 空闲 push）。

约束：

- **放槽时机** = child 进程终态（completed / failed / cancelled），不另等父模型读完。不要做成 Codex `close_agent`。
- 父要补位，必须自己已经不在「等整波 join」里：即 `background: true`，且 `session:{conversation}` 已释放（P1 起后台 job 不占这条车道）。
- 前台 wave **不改**：显式 `background: false` 的 `self` / `explore` 仍 join 全波。省略则走后台滑动窗口。

### 一张工人队列（前台 join + 后台 job）

拆开的是 **等不等**（join vs `jobId`），不是两套并发闸。合并前：同一则消息里 4 条前台 explore + 4 条后台 explore、上限 3，会跑到 **6**（`subagent_sem` 3 + 全局 `running_slots` 3）。A 会话的后台还会用进程全局槽堵住 B 的前台。**现已合成一张按会话 FIFO 工人队列。**

**job 表不跟队列合成。** 前台 join 不发 `jobId`、不进 `job.list` / `await` / 占用 UI。`generating` 已经覆盖本轮 join。合成的只有 **槽 + 等待队列**。

#### 不要并进去的：lead 车道

`RunQueue` 的 `session:{conversation}`（max=1）和 `global:main` / `global:cron` 管的是 **lead 回合**。后台 job **禁止**占这两条。不要把工人队列接进 `acquire()` 那套双闸，避免一不小心把 child 排进 session 车道。

#### 目标结构

| 层 | 职责 | 谁用 |
|----|------|------|
| `RunQueue` | lead 串行 + 全局限流 | 用户回合 / cron |
| **工人队列**（JobSupervisor 内，按会话一把 FIFO） | child 并发上限 | 前台 join、后台 subagent、后台 terminal |
| job 表 | id / 终态 / `claimed` / `content` | 后台 `run_subagent`（self/explore 默认或显式）与 `blockUntilMs` 转后台 |

上限仍是 **`maxParallelSubAgents`**，按 **会话** 计，不是进程全局。会话 A 的 3 格与 B 的 3 格互不占用。暂不加工人全局顶（lead 已有 `maxConcurrentRuns`）。满员 **排队**（现网 wave），不抄 Codex 满了就 `AgentLimitReached`。

`acquire_root` 用 `VecDeque` 叫醒（对齐 `RunQueue` 车道），同一会话里前台 waiter 和后台 waiter 排一队，顺序 = 工具调用顺序。放槽时把根槽**转让**给队头，避免被插队抢走。

#### 借槽规则

| 工人 | 登记 job 表 | 占根槽 | 父行为 |
|------|-------------|--------|--------|
| 前台 `self` / `explore` wave | 否 | 是 | join 到结束，tool result 带 `content` |
| 前台串行 `coder` / `computer` | 否 | 是（1 格） | 同左；不占则后台能和前台 coder 叠加上限 |
| 后台 `run_subagent` | 是 | 是 | 立刻 `jobId`；`done` 不杀 |
| 后台 `terminal.blockUntilMs` | 是 | 是（已在同一张 job 表） | 同左 |
| 前台 `terminal`（无 `blockUntilMs`） | 否 | **否** | 现网；不要顺手并进工人槽 |
| 嵌套 `run_subagent`（`spawn_depth > 0`） | 仅当 `background` | **否**（挂在祖先那 1 格上） | 否则外层占满 N、内层再要槽、外层还在 join → `N=1` 死锁 |

嵌套不计新槽。深度仍用现有 `maxSubAgentSpawnDepth`，不抄 Codex `close_agent`。没有祖先根槽还走嵌套路径 → error 日志并拒绝（不要静默再占一格，也不要空转）。

#### API（落在 JobSupervisor，删掉 per-pass `subagent_sem`）

```text
acquire_root(conversation_id, cap, cancel) -> Option<WorkerLease>
  本会话 running_roots < cap → 立刻 +1
  否则入 FIFO，cancel 则出队返回 None（不占槽）

acquire_nested(conversation_id) -> Result<NestedLease>
  要求该会话已有 ≥1 根槽；不改 running_roots

WorkerLease Drop → running_roots -1，叫醒队头
```

- 前台 wave：`collect_self_fork_wave` 的 `semaphore.acquire_owned` 换成 `acquire_root`。拿到 lease 再发「执行中」，与现在「queued 不显示 running」一致。
- 后台 spawn：先 `register`（list 能看到 queued），再在 `tokio::spawn` 里 `acquire_root`；取消则 `Cancelled`、不占槽。
- 串行 coder/computer：`execute_owned_subagent` 入口 `acquire_root`（lead 路径）；子里再委派走 `acquire_nested`。
- `idleSlots` = `cap - running_roots - waiter_len`（立刻能开跑的空位）。`job.list` 的 `runningCount` 仍只数 **后台** queued+running（占用 UI / 侧栏转圈）。前台根槽只进 `idleSlots` / `poolRunning`，避免模型把 join 中的 explore 当成可 `await` 的 job。

同一则消息里交错前台 + 后台：按工具列表顺序入队，共用 cap。父只 join 前台那几条；没轮到槽的后台继续排，父 `done` 也不杀。

#### 对照

- **Cursor**：一个 `Task`、两档等待。前台整波 join；滑动窗口必须后台。没有「前台一把锁、后台另一张槽表」。
- **Codex**：一张 `AgentRegistry`，每个 spawn 都占槽；满了失败。Pointer 对齐它的 **按会话一把槽**，对齐 Cursor 的 **前台仍 join、不发 id**；排队对齐现网 wave。

#### 落地步骤

1. JobSupervisor：按会话 `{ running_roots, waiters: VecDeque }` 替换全局 `running_slots`；lease RAII；FIFO 单测（取消出队、A 不堵 B、嵌套不占槽、underflow 警告）。
2. 后台 subagent / 后台 terminal 改 `acquire_root`；`idleSlots` 改用共享池。
3. 删 `subagent_sem`；wave 与串行 coder/computer 走同一 `acquire_root`。
4. 嵌套：`OwnedSubagentExecutionInput` 带「已持根槽」；子 `run_subagent` 走 `acquire_nested`。死锁单测：`cap=1` 前台 join + 子再 spawn。
5. 混排单测：同波 2 前台 + 2 后台、`cap=2` → 同时 running 的根槽 ≤ 2；前台 join 返回时后台可仍 queued。
6. 提示词：`idleSlots` 是共享池空位，不是「后台专用」；前台 join 占着时不要按旧数字再补后台。

不改 `job` 的 list/await/claim 契约。不改 session 车道。P2 空闲 push 仍只看 **未认领后台终态**。

### `job` 工具

一个工具、按 `action` 分支（避免 Codex 六工具）：

| action | 含义 |
|--------|------|
| `list` | 本会话后台 job（含子 Agent 与终端）。每条带 **`claimed`**。**不带 `content`** |
| `status` | 单个 job 元数据。**不带 `content`**，**不**认领 |
| `await` | 见下表。**唯一**把终态 `content` 写进本轮并置 **`claimed: true`** |
| `cancel` | 按 id 杀；缺省杀本会话全部后台 |

`await`：

| 字段 | 含义 |
|------|------|
| `jobIds` | 省略 = 本会话全部后台 job（含已完成未认领）。指定则这一组是等待集 |
| `mode` | `any`（默认）：等到本会话至少一条 **未认领终态**。醒后：此刻所有已完成未认领的进 `jobs[]` 并认领。内部工具 running/结果 **不**叫醒父模型。还在跑的只在 `running[]`。`all`：这组全部终态才返回，只认领**尚未 claimed** 的（含会话里其它已完成未认领的）。已认领的不再进 `jobs` |
| `timeoutMs` | 默认 30min；超时不杀、不认领正文 |
| 回包 | `jobs`（本拍认领的正文）、`running`、`runningCount` / `slotCap` / `idleSlots` / `poolRunning`。超时未认领终态 id 才出现在 `unclaimed` |

滑动窗口循环：`await(any)` → 读 `jobs[]` 正文并按 `idleSlots` 再 spawn → 不要默认 `all`。

`await` 与空闲 push **互斥认领**（字段 **`claimed`**，仅终态正文）：谁先把终态交给父模型，谁负责；另一条路径看到已投递则跳过。**第二次 `await` 不得再把同一份 `content` 放进 `jobs`。** `list` / `status` 只给元数据，不带正文、不置 `claimed`。

### JobSupervisor

- 独立 `CancellationToken` 与并发上限（设置项，默认对齐 `maxParallelSubAgents`）。
- **禁止**再拿 `session:{conversation}`。
- 父回合 `done` 不杀 job；用户停会话才 fan-out cancel。
- 终态自动放槽，不另做 `close`。

### 父 Agent 结束了、后台还在跑

「结束」要拆开，不要当成同一件事：

| 发生了什么 | 后台 job | 父模型 | 用户看到 |
|------------|----------|--------|----------|
| 父本轮不再调工具、正常 `done` | **继续跑**（不占 `session:{conversation}`） | 本轮结束。P2：job 终态且结果未被 `await` 认领 → **一轮**空闲 push，把摘要交回 lead | 子任务行仍「后台执行中」；侧栏该会话保持转圈，直到没有 running job |
| 用户又发了一句 | **继续跑** | 新一轮 lead 可 `job.list` / `await`；不要取消后台 | 主会话能聊，底下子任务还在动 |
| 用户点停止 | **全部 cancel** | 本轮中止 | 「已取消」 |
| 立即发送 / 结束等待（软取消） | **继续跑** | 本轮中止（`job.await` 也结束） | 后台行仍「后台执行中」 |
| 应用退出 / 进程没了 | **一起没**（P0 不恢复） | — | 下次打开不再是 running |

这是 Cursor 后台 `Task` 的用法，也是 Codex #15723 要补的：父闲了 child 还在时，**默认不杀**；缺的是做完怎么把结果交回（P2 push），而不是把后台又做成 join。

不要在父 `done` 时偷偷 `await all`——那等于禁止后台。

### 谁决定该等还是不用等

**宿主不猜用户意图。** 「这份结果要不要进本轮回答」只有当时在编排的父模型知道；Cursor 用 `run_in_background`，Codex 用要不要调 `wait_agent`，都是模型选的。Pointer 分三层，后一层不能推翻前一层已经选过的档：

| 层 | 谁 | 做什么 |
|----|----|--------|
| 1. 默认 | **宿主** | **`self` / `explore`**：不传 `background` = 后台。**`coder` / `computer` / `terminal`**：不传 = 前台 join。 |
| 2. 开前台 | **父模型** | `self` / `explore` 写 `background: false` = 本条必须 join 到结束。 |
| 3. 本轮要不要结果 | **父模型** | 开了后台之后：还要 `job.await` 才把 `content` 拿进本轮；不再调工具、直接 `done` = 本轮不等，结果走空闲 push。 |
| 4. 纠错 | **用户 / UI / push** | Composer **停止** = lead + 本会话全部后台。**立即发送 / 结束等待** = 只停同步（含 `job.await`），后台继续。行上 **结束任务** = 只杀该 `jobId`。侧栏和子任务行永远按真实 running 画。父说完了但 job 还在：空闲 push 再给 lead 一轮，把摘要补回来。 |

「需要结果才能回答时就 await 或 `background: false`」是给模型的**提示词规则**，不是宿主分类器。宿主无法可靠判断「用户问的是不是必须等探索结束」——去解析正文再偷偷 join，会把后台废掉。

宿主只做这几件硬事（不做语义判断）：

- `self` / `explore` 没标或 `true` → 后台；显式 `false` → 必须 join。
- 标了后台 → 禁止在 `done` 时自动 `await all`。
- 界面以 job 状态为准：还在跑就不能让界面看起来像全部完成。
- 用户点 **停止** → cancel lead + 全部后台，不询问模型。
- 立即发送 / 行上「结束等待」→ 软取消（`cancelBackgroundJobs=false`）：结束同步与 await，后台继续。
- 行上「结束任务」→ 只 `cancel` 该 `jobId`。

模型若开了后台又对用户说「已经全部完成」，以界面为准（仍显示后台执行中）；P2 push 后 lead 再改口汇总。不在宿主里拦 `done`。

空闲 push 仍是 **一轮、可合并、与 await 互斥认领**。取消 / 停止 / 进程退出不 push。

### 空闲 push（P2，已落地）

仅当：job 为 **Completed / Failed** **且** 该会话没有进行中的 lead 轮 **且** 这条完成尚未被 `await` 认领。

实现：`idle_job_push` 挂在 JobSupervisor 终态回调 + lead `on_run_finished/failed/cancelled`。约 **500ms** debounce 后认领本会话全部未认领终稿，`TriggerSource::Internal`（`internal_label=idle_job_push`）在**同一 `conversation_id`** 再开一轮。注入用户消息：`content` = 完整终稿（进模型）；`uiBindings.bubbleText` =「后台任务已完成。」（仅气泡）；`hostKind=idle_job_push`。与 `await` 互斥 `claimed`。dispatch 失败会 `unclaim` 以便重试。

取消、停会话（job → Cancelled）、进程退出：**不** push。终端默认仍是前台；只有显式 `blockUntilMs` 的终端 job 才会进这张表。

## 分期

| 阶段 | 内容 |
|------|------|
| **P0** | JobSupervisor；`terminal.blockUntilMs`；`job` list/status/await/cancel；Unix `killpg` / Windows `taskkill /T` |
| **P1** | `run_subagent.background` 仅 `self`/`explore`；`job.await` 支持 `any`/`all`；终态立刻放槽；UI 沿用 `SubAgentFrame` |
| **P1.5** | **一张工人队列**（已落地）：删 `subagent_sem` + 全局 `running_slots`；按会话 FIFO；前台借槽不登记；嵌套不计新槽；串行 coder/computer 也借 1 格 |
| **P2** | **空闲合并 push（已落地）**；`resume` / `taskId` 续跑；自定义 agent `is_background` |
| **P3** | 重叠写入的 worktree 隔离；`coder` 后台（须隔离）；不做云 VM |

**初版已落地**：JobSupervisor、`run_subagent.background`（self/explore）、`terminal.blockUntilMs`、`job` 工具、侧栏转圈 / 「后台执行中」、同一会话空闲合并 push。

## UI / 文案

- 后台子任务仍挂在宿主 `run_subagent` 行下，统计行 +「思考中」规则与前台相同。
- 界面只说「后台执行中 / 已完成 / 已取消」，不要写 jobId、lane、supervisor；不要把句柄 JSON 当「结果」展开给用户。
- 侧栏会话在有后台 job 时保持转圈，直到该会话 **没有** running job（用户应能边聊边看）。
- 停止按钮 = 本会话 `generating` **或** 后台占用 > 0。不是只靠 `Done`。
  - `generating`：发出本轮时点亮，`Done` / 停止 / 报错 / 队列对账清掉。
  - 占用：`background_jobs` 事件、`job` 工具后、`Done.backgroundRunningCount`、启动/切前台时的队列快照 `backgroundJobs`。
  - 不要用落盘里仍 `running` 的宿主行点亮按钮。JobSupervisor 不落盘，进程一关 job 就没了；快照里没有的会话占用必须是 0，宿主行收成中断。

## 提示词（落地时）

工具说明用英文、短行、不提文件名：

- `self` / `explore` 默认后台。本轮下一步被结果堵住且不打算 `await` 时写 `background: false`。
- 后台后用 `job.await`，不要结束本轮干等；能结束则结束，等空闲 push。
- 若本轮先结束：后台继续；不要对用户说已经全部完成。结果走之后的 `await` 或完成后的那一轮汇总。
- 要打满并发、下一任务又依赖已完成结果：用 `await` `mode=any`，本拍已就绪的全部正文都在 `jobs[]`，再 spawn，不要 `all`。
- 任务清单已齐、只要全部摘要：同一则消息里一次列出（可超过并发上限，宿主排队补位），再 `await` `mode=all`。
- 并行探索：同一则消息里多个 `run_subagent`（默认后台）；要立刻再说话就直接说，不要假称已全部完成。
- **终端**省略 `blockUntilMs` 仍前台。

## 非目标（P0）

- 崩溃后恢复正在跑的 child / PTY。
- 后台 `computer`、elevated 终端、交互 stdin。
- 把 Workspace 用户终端当成 Agent job。
