# 异步子 Agent 与后台终端

> 实现进度：初版已落地 JobSupervisor、`run_subagent.background`（self/explore）、`job` list/status/await/cancel。终端 `blockUntilMs` 与空闲 push 尚未做。
>
> **默认必须与现网一致：不传后台参数 = 仍 join 到结束。**
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
| 停一条 | `interrupt`（对正在跑的 async） | `interrupt_agent`（不停掉上下文） | 只能停整轮会话 |
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
2. **wait 要写清契约。** Codex `wait_agent` 等的是**调用者邮箱里任意一条更新**（适合滑动窗口），不是 wait-all；嵌套完成只投递直接父级（#32203 / #41142）。Pointer 拆成显式 `mode`：`any` = 先完成先返回（补位）；`all` = 这组齐了再返回。按 `jobIds` 过滤，超时后 job **继续跑**。
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
| 省略 / `false` | 与现网相同：阻塞到结束，tool result 带 `content` | Cursor Foreground |
| `true` | 立刻 `{ jobId, status: "running" }`；child 在 JobSupervisor 里跑 | Cursor Background + Codex `spawn_agent` |

P0 仅 `self` / `explore`。其它 agentId 传 `background: true` 时工具失败并说明须前台 join。

### `terminal.blockUntilMs`

| 值 | 行为 | 对齐 |
|----|------|------|
| 省略 | 等到命令结束（现网） | Codex 前台 exec |
| `0` | 立刻返回 `jobId` | Codex 后台 session |
| `N>0` | 最多等 N ms，未结束则 detach 成 job | Codex yield / 超时后转后台 |

P0 不含 elevated、需交互 stdin 的命令。Workspace 用户终端仍独立。

### 滑动窗口：谁完成谁让槽

并发上限是 **同时 running 的槽数**，不是「这一波要等齐」。先结束的 child **立刻退出并放槽**，不必等最慢的兄弟。

| 场景 | 现网 owned-wave | 后台滑动窗口 |
|------|-----------------|--------------|
| 同一则消息里已列出 10 个任务、上限 4 | 信号量会补位（4 跑完再接下一个），但父 LLM 仍卡到 10 个全 join | JobSupervisor 同样补位；父已拿到 10 个 `jobId`，可去干别的 |
| 下一任务要看**已完成者的结果**才知道 | 做不到：父被整波 join 堵住，空槽闲着 | `job.await(any)` 先完成的先把 `content` 交回；父立刻 `run_subagent` 补一个。其它仍在跑的继续占槽 |

这就是 Cursor 后台 `Task` 和 Codex「邮箱有更新就醒」（不要 wait-all）真正省掉的浪费：慢任务不必拖住快任务腾出来的槽。

约束：

- **放槽时机** = child 进程终态（completed / failed / cancelled），不另等父模型读完。不要做成 Codex `close_agent`。
- 父要补位，必须自己已经不在「等整波 join」里：即 `background: true`，且 `session:{conversation}` 已释放（P1 起后台 job 不占这条车道）。
- 前台 wave **不改**默认：不传 `background` 仍 join 全波，行为与现网一致。

### `job` 工具

一个工具、按 `action` 分支（避免 Codex 六工具）：

| action | 含义 |
|--------|------|
| `list` | 本会话后台 job（含子 Agent 与终端） |
| `status` | 单个 job；终态带截断后的 `content` / exit |
| `await` | 见下表 |
| `cancel` | 按 id 杀；缺省杀本会话全部后台 |

`await`：

| 字段 | 含义 |
|------|------|
| `jobIds` | 省略 = 本父挂起的 running。指定则只等这一组 |
| `mode` | `any`（默认）：**任意一条**终态就返回该条 `content`，其余继续跑。`all`：这组全部终态才返回 |
| `timeoutMs` | 默认 30min；超时不杀，返回仍 running 的 id |

滑动窗口循环：`await(any)` → 读结果 → 若还有工作则再 `run_subagent(background)` → 再 await。不要默认 `all`，否则又回到「等最慢的」。

`await` 与空闲 push **互斥认领**：谁先把终态交给父模型，谁负责；另一条路径看到已投递则跳过。

### JobSupervisor

- 独立 `CancellationToken` 与并发上限（设置项，默认对齐 `maxParallelSubAgents`）。
- **禁止**再拿 `session:{conversation}`。
- 父回合 `done` 不杀 job；用户停会话才 fan-out cancel。
- 终态自动放槽，不另做 `close`。

### 父 Agent 结束了、后台还在跑

「结束」要拆开，不要当成同一件事：

| 发生了什么 | 后台 job | 父模型 | 用户看到 |
|------------|----------|--------|----------|
| 父本轮不再调工具、正常 `done` | **继续跑**（不占 `session:{conversation}`） | 本轮结束。P2：job 终态且结果未被 `await` 认领 → **一轮**空闲 push，把摘要交回 lead | 子任务行仍「后台运行」；侧栏该会话保持转圈，直到没有 running job |
| 用户又发了一句 | **继续跑** | 新一轮 lead 可 `job.list` / `await`；不要取消后台 | 主会话能聊，底下子任务还在动 |
| 用户点停止 | **全部 cancel** | 本轮中止 | 「已取消」 |
| 应用退出 / 进程没了 | **一起没**（P0 不恢复） | — | 下次打开不再是 running |

这是 Cursor 后台 `Task` 的用法，也是 Codex #15723 要补的：父闲了 child 还在时，**默认不杀**；缺的是做完怎么把结果交回（P2 push），而不是把后台又做成 join。

不要在父 `done` 时偷偷 `await all`——那等于禁止后台。

### 谁决定该等还是不用等

**宿主不猜用户意图。** 「这份结果要不要进本轮回答」只有当时在编排的父模型知道；Cursor 用 `run_in_background`，Codex 用要不要调 `wait_agent`，都是模型选的。Pointer 分三层，后一层不能推翻前一层已经选过的档：

| 层 | 谁 | 做什么 |
|----|----|--------|
| 1. 默认 | **宿主** | 不传 `background` = 前台 join。这是安全默认，模型没表态就等齐。 |
| 2. 开后台 | **父模型** | 某个 `run_subagent` 写 `background: true` = 这条工具可以不等它结束。 |
| 3. 本轮要不要结果 | **父模型** | 开了后台之后：还要 `job.await` 才把 `content` 拿进本轮；不再调工具、直接 `done` = 本轮不等。 |
| 4. 纠错 | **用户 / UI / P2** | 停止 = 全取消。侧栏和子任务行永远按真实 running 画。父说完了但 job 还在：P2 空闲 push 再给 lead 一轮，把摘要补回来。 |

「需要结果才能回答时就 await」是给模型的**提示词规则**，不是宿主分类器。宿主无法可靠判断「用户问的是不是必须等探索结束」——去解析正文再偷偷 join，会把后台废掉。

宿主只做这几件硬事（不做语义判断）：

- 没标 `background` → 必须 join。
- 标了 `background` → 禁止在 `done` 时自动 `await all`。
- 界面以 job 状态为准：还在跑就不能让界面看起来像全部完成。
- 用户点停止 → cancel，不询问模型。

模型若开了后台又对用户说「已经全部完成」，以界面为准（仍显示后台运行）；P2 push 后 lead 再改口汇总。不在宿主里拦 `done`。

空闲 push 仍是 **一轮、可合并、与 await 互斥认领**。取消 / 停止 / 进程退出不 push。

### 空闲 push（P2）

仅当：job 终态 **且** 该会话没有进行中的 lead 轮 **且** 这条完成尚未被 `await` 认领。

合成一条内部消息（对用户显示为简短「后台任务已完成」即可，不要把 child thoughts 塞进父上下文），触发 **一轮** lead。多条近同时完成合并。

取消、停会话、进程退出：**不** push。

## 分期

| 阶段 | 内容 |
|------|------|
| **P0** | JobSupervisor；`terminal.blockUntilMs`；`job` list/status/await/cancel；Unix `killpg` / Windows `taskkill /T` |
| **P1** | `run_subagent.background` 仅 `self`/`explore`；`job.await` 支持 `any`/`all`；终态立刻放槽；UI 沿用 `SubAgentFrame` |
| **P2** | 空闲合并 push；`resume` / `taskId` 续跑；自定义 agent `is_background` |
| **P3** | 重叠写入的 worktree 隔离；`coder` 后台（须隔离）；不做云 VM |

**初版已落地**：JobSupervisor、`run_subagent.background`（self/explore）、`job` 工具、侧栏转圈 / 「后台运行」。不含终端 `blockUntilMs`、空闲 push。

## UI / 文案

- 后台子任务仍挂在宿主 `run_subagent` 行下，统计行 +「思考中」规则与前台相同。
- 界面只说「后台运行 / 已完成 / 已取消」，不要写 jobId、lane、supervisor。
- 侧栏会话在有后台 job 时保持转圈，直到该会话 **没有** running job（用户应能边聊边看）。

## 提示词（落地时）

工具说明用英文、短行、不提文件名：

- 默认前台。只要结果才能往下规划时不要 `background`。
- `background: true` 后用 `job.await`，不要结束本轮干等。
- 若本轮先结束：后台继续；不要对用户说已经全部完成。结果走之后的 `await` 或完成后的那一轮汇总。
- 要打满并发、下一任务又依赖已完成结果：用 `await` `mode=any`，拿到一条就再 spawn，不要 `all`。
- 任务清单已齐、只要全部摘要：同一则消息里一次列出（可超过并发上限，宿主排队补位），再 `await` `mode=all`。
- 并行探索：同一则消息里多个 `run_subagent`；要立刻再说话才后台。

## 非目标（P0）

- 崩溃后恢复正在跑的 child / PTY。
- 后台 `computer`、elevated 终端、交互 stdin。
- 把 Workspace 用户终端当成 Agent job。
