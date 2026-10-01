# Cron one-shot（延迟一次）+ 软完成设计

## 目标

对齐 Hermes cron 的 **一次性延迟 / 定点执行**，并在 Pointer 用 **软完成** 保留任务行与「查看会话」入口：

| Schedule | 行为 |
|----------|------|
| `30m` / `2h` / `1d` | 从现在起延迟一次触发 |
| ISO `2026-07-22T09:00:00`（无 offset = 本地） | 到点触发一次 |
| 现有 preset / 6 段 cron | 周期，行为不变 |

执行后：**不删行**；`enabled=0`，`next_run_at_ms=NULL`，写 `last_run_at_ms`；Automation 仍可打开 `cron:{id}:{yyyymmdd}` 会话。

## 非目标（MVP）

- Hermes `every 2h` 空格别名（可后续映射到现有 `every_2_hours`）
- `repeat.times=N` 有限次重复
- once 任务 re-enable「再跑一次」（MVP：**拒绝 enable once 已完成/禁用任务**，提示新建）
- 缩短 60s tick；接受最多约 1 分钟延迟
- 硬删 / 独立 reminders 表 / 归档表

## 方案

采用 **`schedule_kind` + `schedule_raw`（方案 A）**，不用 `@once` 哨兵伪 cron，也不新建表。

### 存储

`cron_jobs` 新增（`add_column_if_missing`）：

| 列 | 类型 | 说明 |
|----|------|------|
| `schedule_kind` | TEXT NOT NULL DEFAULT `'cron'` | `'cron'` \| `'once'` |
| `schedule_raw` | TEXT | 用户/模型原文（展示与审计） |

once 创建时：

- `cron_expr` 存字面量 `@once`（**禁止**传入 `cron::Schedule`；仅占位）
- `next_run_at_ms` = 绝对触发时间（UTC ms）
- `enabled = 1`

`CronJobRecord` / `CronJobView` / TS `CronJob` 增加 `scheduleKind`、`scheduleRaw`。

### 解析

`tools/cron_job/schedule.rs` 改为返回：

```rust
pub enum ParsedSchedule {
    Recurring { cron_expr: String },
    Once { fire_at_ms: i64 },
}
```

规则（顺序）：

1. 相对 once：`(?i)^(\d+)(m|h|d)$` → `now_local + duration`（毫秒）
2. ISO once：`chrono` 解析 RFC3339 或 `NaiveDateTime`（无 offset → Local）→ `fire_at`；`fire_at <= now` 则 Err
3. 既有 recurring 路径 → `Recurring { cron_expr }`

`describe_schedule`：once 用「N 分钟后一次」/「YYYY-MM-DD HH:MM 一次」；周期逻辑不变。

创建校验：`next` 必须存在且 `> now`。

### 生命周期

```
create(once)
  → insert(kind=once, next=fire_at, enabled=1)

Scheduler::tick → list_due → dispatch_job
  → mark_ran(once):
       enabled=0, next_run_at_ms=NULL, last_run_at_ms=ran_at
       // 不 delete；防同 slot 重入

Automation list：
  - enabled=1 → 进行中
  - kind=once && enabled=0 → 已完成（可「查看会话」）
  - kind=cron && enabled=0 → 已暂停（现有）

set_enabled(true) on once：
  → Err（「一次性任务不可重新启用，请新建」）

用户手动 delete → 现有硬删（会话 transcript 仍可留在库中）
```

deliver / `[SILENT]` / `ImDeliverHook` / 会话模型：**不变**。

### API / 工具 / UI

| 入口 | 变更 |
|------|------|
| `cron_job` 工具 | `schedule` 支持 once 字符串；返回带 `scheduleKind` |
| create HTTP / Tauri | 优先接受 `schedule` 字符串并走统一 `parse_schedule`；兼容现有 `cronExpr`（视为 recurring） |
| `cron_job.md` + `general/AGENT.md` | 文档：提醒类用 `30m`；跑完软完成可查会话 |
| Automation UI | 增加「延迟一次」「指定时间」；列表标注一次性 / 已完成 |

### 跨入口

APP（Tauri）与 WEB（server）共用同一 DB + 同一解析；两端 create 均走 core 解析。

## 测试计划

1. **`schedule.rs`**：`30m`/`1d`/ISO 成功；过去时间失败；`daily@9:30` 回归
2. **`cron_jobs.rs`**：insert once → `list_due` → `mark_ran` → 行仍在、`enabled=0`、不再 due；recurring `mark_ran` 仍推进 next
3. **`cron_job` 工具**：`create` + `schedule=30m`（in-memory store）
4. **enable once**：`set_enabled(true)` 返回错误
5. **前端**（若改 picker）：`cronSchedule.test.ts` 覆盖 once 构建/描述

端到端（仓库内）：工具 create → 人为把 `next_run_at_ms` 设为过去 → 调 `list_due` + `mark_ran`（或可测的 tick 辅助）→ 断言软完成；不要求真等 30 分钟或真跑 LLM。

## 风险

| 风险 | 缓解 |
|------|------|
| `@once` 误入 `next_run_ms` | `mark_ran` / `next_run_ms` 对 `schedule_kind=once` 短路 |
| 派发失败仍软完成 | 与现周期一致（dispatch 后即 mark）；失败靠会话/日志排查，可手动新建 |
| tick 最多晚 ~60s | 提示词说明；不改 tick |

## 文件索引

- `crates/pointer-core/src/tools/cron_job/schedule.rs`
- `crates/pointer-core/src/tools/cron_job/mod.rs` + `prompts/cron_job.md`
- `crates/pointer-core/src/conversation_store/cron_jobs.rs` + `mod.rs`（migration）
- `crates/pointer-core/src/scheduler.rs`（仅当 mark 需读 kind）
- `server` / `src-tauri` create 路径
- `src/lib/cronSchedule.ts` + Automation 面板
- `docs/internals/trigger-dispatcher.md`（一行说明 once）
