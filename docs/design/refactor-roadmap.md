# 代码重构路线图

基于 2026-06 代码审查（文件行数、函数行数、逻辑复杂度）。按优先级分步完成，每步独立可合并。

**原则**

- 行为不变：重构以提取模块为主，不顺便改产品逻辑
- 小步 PR：每个优先级尽量单独提交、可回滚
- 测试：Rust 侧 `cargo test -p pointer-core`；前端改 stream 路由后需手动验证主会话 + sub-agent 流式 UI

---

## 审查快照（起点）

| 指标 | 前端 | Rust `pointer-core` |
|------|------|---------------------|
| 文件 >1000 行 | 3 | 8 |
| 函数 >100 行 | 1（567 行） | 30+ |
| 复杂度热点 | `chat.ts` `handleEventInner` | `agent_tool_pass.rs`、`models.rs`、`tools/file.rs` |

---

## P0 — 前端 stream 事件路由（`chat.ts`）

**问题**：`handleEventInner` ~567 行，单 switch 处理 ~30 种 `StreamEvent`，分支密度全项目最高。

**目标结构**

```
src/stores/
  chat.ts
  chat/
    helpers.ts
    taskBoard.ts                 # 任务板状态合并 + manager
    terminalLive.ts              # 终端 live popup 管理
    desktopNotice.ts             # 桌面注入行自动移除
    streamHandlers/
      ...
```

**步骤**

- [x] 建立本路线图文档
- [x] 提取 `StreamHandlerContext` + `dispatchStreamEvent`
- [x] 按域拆分 handler 模块（第一版）
- [x] 为关键 handler 补单元测试（vitest）
- [x] 将 task-board / terminal-live / desktop-notice 下沉到 `chat/` 子模块

**验收**

- `chat.ts` 不再包含巨型 switch 实现体
- 新增 `StreamEvent` kind 时只需改 `dispatch.ts` + 对应 handler 文件
- IM / sub-agent traceId / 压缩 / 工具卡 UI 行为与重构前一致

---

## P1 — Rust 工具执行 pass（`agent_tool_pass.rs`）

**问题**：834 行；`run_agent_tool_pass`（256 行）、`execute_tool_invocation`（202 行 if-else 工具链）、`record_tool_exec_outcome`（110 行）。

**目标结构**

```
chat_service/agent_tool_pass/
  mod.rs           # run_agent_tool_pass 编排
  approval.rs
  dispatch/
    mod.rs
    terminal.rs
    web_search.rs
    media.rs
    subagent.rs
    registry.rs
  outcome.rs
  types.rs         # ToolPassContext（含 persist_transcript）
```

**步骤**

- [x] 引入 `ToolPassContext`，收拢 `persist_transcript` 等参数（sub-agent 路径已用 `persist_transcript = sub.is_none()`）
- [x] 将 `execute_tool_invocation` 改为 dispatch 表 / 分文件实现
- [x] 拆分 `record_tool_exec_outcome` → `outcome.rs`
- [x] 拆分 `run_approval_gate` → `approval.rs`
- [x] `cargo check -p pointer-core` 通过

---

## P1 — Rust 模型层（`models.rs`）

**问题**：3586 行；类型、`StreamEvent`、`ModelSettings` 默认值、`make_openai_messages`、测试混在一文件。

**目标**

```
models/
  message.rs
  conversation.rs
  settings.rs
  stream_event.rs
  openai_convert.rs
  mod.rs           # pub use 保持对外 API
```

**步骤**

- [x] 先拆 `stream_event.rs` + `settings.rs`（只移动，不改名）
- [x] 再拆 `openai_convert.rs` + 测试模块
- [x] 确认 `pointer-core` 及 Tauri 边界编译通过

---

## P2 — 工具文件层（`tools/file.rs`）

**问题**：2559 行，6 种 file 工具 + 路径解析堆叠。

**目标**：`tools/file/{mod,path,read,write,edit,list,glob,grep}.rs`

**步骤**

- [x] 先提取 `path.rs`
- [x] 逐个迁移 `execute_*_payload`
- [x] 保持 `register_all` 对外签名不变

---

## P2 — Supervisor / Sub-agent 循环

**问题**：`run_supervisor_chat`（417 行）、`run_sub_agent`（305 行）与 `single_agent` 循环重复。

**目标**：共用 `AgentLoopRunner` 或 phase 函数（plan / execute / finalize）。

**步骤**

- [x] 梳理 lead / sub / supervisor 三处差异点（见 `agent_round_lifecycle.rs` 模块注释）
- [x] 提取共享 stream → tool_pass 骨架（`check_loop_guards` / `resolve_post_assistant_action` / `finish_tool_round_cycle`）
- [x] 保留 sub-agent `local_history` + `persist_transcript` 语义

---

## P3 — 前端 Settings 大组件

| 文件（重构前） | 行数 | 重构后 |
|------|------|--------|
| `SettingsDialog.vue` | 1591 | ~282（shell + 路由） |
| `ChannelSettingsPanel.vue` | 1181 | ~540（模板/样式） + `useChannelSettingsForm.ts` |

**目标结构**

```
src/composables/
  useSettingsDialogForm.ts
  useChannelSettingsForm.ts
src/components/settings/
  SettingsDialog.vue
  panels/
    AssistantSettingsPanel.vue
    GenerationSettingsPanel.vue
    AgentSettingsPanel.vue
    AccountSettingsPanel.vue
    RuntimeSettingsPanel.vue
  ChannelSettingsPanel.vue
```

**步骤**

- [x] 按设置 Tab 拆 `SettingsDialog` 子面板组件
- [x] 共享 `useSettingsDialogForm` composable
- [x] 提取 `useChannelSettingsForm`（Channel 面板逻辑与生命周期）

**验收**

- `npm run build` 通过
- 设置页各 Tab 保存/连接/扫码行为与重构前一致

---

## 已完成的相关修复（非本路线图范围）

- Sub-agent `local_history` 不再 flush 到主会话 DB（`persist_transcript = sub.is_none()`）

---

## P4 — Agent 编排层 Context 收拢（>10 参函数）

**问题**：`chat_service` 内 21 个函数参数 >10，编排层传参重复、难维护。

**目标结构**

```
chat_service/context/
  session.rs, budget.rs, llm.rs, transcript.rs
  post_assistant.rs, stream_round.rs, tool_pass.rs
  prompt.rs, loop_ctx.rs, chat_run.rs
scripts/count_fn_params.py   # 参数扫描 baseline
```

**步骤**

- [x] Step 0：context 类型骨架 + 参数扫描脚本 + 本章节
- [ ] Step 1：`PostAssistantContext`（7 个函数 → ≤6 参）
- [ ] Step 2：`StreamRoundContext`（3 个函数 → ≤6 参）
- [ ] Step 3：`ToolPassContext`（5 个函数 → ≤6 参）
- [ ] Step 4：Prompt context（2 个函数 → ≤6 参）
- [ ] Step 5：Loop 入口（4 个函数 → ≤6 参）
- [ ] Step 6：`run_chat_inner`（1 个函数 → ≤6 参）

**验收**

- `python3 scripts/count_fn_params.py crates/pointer-core/src/chat_service` 输出 0
- `cargo test -p pointer-core` 通过
- sub-agent `persist_transcript = sub.is_none()` 语义不变

---

## 进度记录

| 日期 | 项 | 说明 |
|------|-----|------|
| 2026-06-14 | P0 v1 | `chat/streamHandlers/*` 拆分 `handleEventInner` |
| 2026-06-14 | P1 v1 | `agent_tool_pass/` 模块拆分 + dispatch 路由 |
| 2026-06-14 | P2 v1 | `tools/file/` 模块拆分 + `agent_round_lifecycle` 共享循环阶段 |
| 2026-06-14 | P1 v2 | `models/` 拆分（message / conversation / settings / stream_event / openai_convert） |
| 2026-06-14 | P0 v2 | taskBoard / terminalLive / desktopNotice 模块 + vitest |
| 2026-06-14 | P3 v1 | Settings 拆分：`panels/*` + `useSettingsDialogForm` + `useChannelSettingsForm` |
| 2026-06-14 | 文档 | 创建本路线图 |
