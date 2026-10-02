# 本轮文件修改 Review（turn baseline）

[English](../../en/developer/turn-file-baseline-review.md) | 简体中文

## 行为

1. **工具结果与 UI 拆开**：`file_edit` / `file_write` 的 tool result **不含** `diff_lines`（`path` / `success` / `stats`；edit 另有 `replaced`，write 另有 `bytesWritten` / `created`）。工具行片段 Diff 由前端用参数里的 `oldString`/`newString` 现算；整文件净 diff 由 turn baseline 懒算。
2. **轮次基线**：同一会话**主 Agent 用户轮次**内，某路径**第一次** `file_edit` / `file_write` 前，把当时全文（新建则为空）写入应用数据目录。
   - `turn_id` 必须是 lead 用户消息 id；子 Agent 的 host stub / scoped 用户行**不能**当作锚点（否则会出现「未找到本轮修改前快照」）。
   - 子 Agent 内的 `file_edit` / `file_write` 同样走 registry 的 `TurnBaselineGuard`，与主 Agent 共用上述 `turn_id`。
3. **对话页脚**：每一轮结尾列出该轮改过的文件（含 scoped 子 Agent）。
   点击打开右侧工作区面板。
   对比「本轮基线 ↔ 其后同一路径的下一份基线」；没有下一份则对比磁盘。
   交互与文案见 [`../ui/turn-change-summary.md`](../ui/turn-change-summary.md)。
4. **右侧栏主导航**：文件夹 / Git 图标（hover：工作区文件、变更文件）。本轮净 diff 走预览 Tab。
5. **Git「变更文件」预览**：整文件 structured diff（可折叠未改行），不是 `git diff` hunk 文本。
   - 有未暂存 / 未跟踪 → index（或空）↔ 工作区磁盘
   - 仅暂存 → HEAD ↔ index
   - `MM` 优先未暂存侧；单侧上限约 2MB
   - API：`get_workspace_git_diff` / `GET /api/workspace/git/diff`（可选 `status`）→ `diffLines` / `diffStats` / `mode`

## 存储

`{app_data}/turn-baselines/{conversation_id}/{turn_id}/{sha256(path)}.txt`

- `turn_id` = 该轮 lead 用户消息 id（与 task board main-turn 锚点一致；`latest_real_user_message_id` 会跳过 scoped / 合成用户行）
- 单文件上限 5MB；超限跳过并打 warn
- 捕获依赖 registry 在 `file_edit` / `file_write` 调用时设置的 `TurnBaselineGuard`
- 旁路：
  - `.path`：绝对路径（调试 / 无 `summary.json` 时页脚回退，无 +/-）
  - `summary.json`：页脚摘要（UI 冻结该轮时写入一次，内容与界面列表相同）

## API

- Tauri：`get_turn_file_diff`（`spawn_blocking`，避免阻塞 UI 异步运行时）
- HTTP：`GET /api/workspace/turn-file-diff?conversationId=&turnId=&workspaceRoot=&path=`

返回 `diffLines` / `diffStats` / `baselineMissing` / `created`（camelCase）。

- Tauri：`save_turn_file_changes` / `list_turn_file_changes`
- HTTP：
  - `POST /api/workspace/turn-file-changes` body `{ conversationId, turnId, files }`
  - `GET /api/workspace/turn-file-changes?conversationId=&turnIds=id1,id2`

`files` 为 `[{ path, kind, adds, dels }]`（camelCase）。
只读写 baseline 目录，**不** `load_messages`、**不** hydrate scoped。

### 找「下一轮基线」时不要全量读消息

对比「本轮基线 ↔ 其后同路径下一份基线」需要后续 **lead 用户 turn id** 列表。
实现必须用轻量查询（`message_id` + `content` + `is_scoped` / `position`），
**禁止**对整段会话 `load_messages` 再反序列化全部 `payload`。

长会话里 `conversations.db` 单会话可达数万行、百 MB 级 payload；
全量加载会让点击「变更文件」打开右侧 diff 明显变慢（短会话不易察觉）。

### 文件写入时的回合 id 不要每次查全量

`turn_id` 就是本轮 lead 用户消息 id，一轮里不变。
`run_chat` 开始时从已在内存的工作集取一次并记住（按 `conversation_id`）。
之后同一次 run 的 `file_edit` / `file_write`（含子 Agent）直接复用，再设 `TurnBaselineGuard`。

进程重启或写入时还没有记住：按 `position` 从尾部读 `message_id` + `content`
（`role=user` 且 `is_scoped=0`），跳过合成用户行，命中后同样记住。
不要用 `is_system_generated`，不要 `load_messages`。
没有真实用户消息时用 `orphan-{conversation_id}`，并且不要把这个占位 id 记住。

## 性能

| 路径 | 预期成本 | 禁止 |
|------|----------|------|
| `ensure_baseline` | 每路径每轮一次全文写盘（已有） | — |
| `save_turn_file_changes` | 每轮冻结成功写一次 `summary.json` | 在每次 file_edit 写摘要；失败不得挡 UI |
| `list_turn_file_changes` | 读 `summary.json`（或 `.path` 回退）；按 turnIds 批量 | 为页脚拉消息 / scoped |
| `turn_file_diff` | 读两份全文 + diff；点击 Review 才触发 | 页脚列表里批量算整文件 diff |
| 写入时的 `turn_id` | 本轮复用已记住的 lead 用户消息 id；未记住时尾部轻量查询一次 | `load_messages`；用 `is_system_generated` 当锚点 |

页脚展示策略见 [`../ui/turn-change-summary.md`](../ui/turn-change-summary.md)「性能」。

## 跨端

桌面与 Web 共用同一 core 逻辑与 API；两端都需能访问会话工作区磁盘路径。

## 已知问题（先不做）

同一工作区、不同会话改同一文件时，**页脚文件列表不串**（按会话消息 / `{conversation_id}/{turn_id}` 基线）。
Review 在「本会话后面没有同路径基线」时右侧读**磁盘**，会串。

例：

1. A 改文件 X（本会话此后未再改 X → 右侧 = 磁盘）
2. B 改同一文件 X
3. A 再次改 X

步骤 2 前后，再打开 A **第一次**那轮的 diff，右侧已经是 B 写过的内容，和步骤 1 刚结束时不一样。
步骤 3 之后，A 第一次那轮的右侧改为 A 第二次改前的基线（当时磁盘是 B 的版本），不再跟实时磁盘走；A 第二次那轮才是「B 的版本 → A 又改之后」。

未决：历史轮是否应冻一份「改完」快照，避免被其它会话的磁盘改写拖走。
