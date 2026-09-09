# 本轮文件修改 Review（turn baseline）

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

## API

- Tauri：`get_turn_file_diff`（`spawn_blocking`，避免阻塞 UI 异步运行时）
- HTTP：`GET /api/workspace/turn-file-diff?conversationId=&turnId=&workspaceRoot=&path=`

返回 `diffLines` / `diffStats` / `baselineMissing` / `created`（camelCase）。

### 找「下一轮基线」时不要全量读消息

对比「本轮基线 ↔ 其后同路径下一份基线」需要后续 **lead 用户 turn id** 列表。
实现必须用轻量查询（`message_id` + `content` + `is_scoped` / `position`），
**禁止**对整段会话 `load_messages` 再反序列化全部 `payload`。

长会话里 `conversations.db` 单会话可达数万行、百 MB 级 payload；
全量加载会让点击「变更文件」打开右侧 diff 明显变慢（短会话不易察觉）。

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
