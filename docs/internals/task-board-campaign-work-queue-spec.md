# Task Board Campaign + Work Queue — Technical Spec (v1 design notes)

> **v4 runtime (2026):** Implemented behavior follows **[Task Board v4](task-board-v2-schema.md)**.
> When reading this doc, map: `board[]` → `global_milestones[]`; `campaign_id` → `store_id`; no `batch_id`;
> row evidence → `remark` / `work_item.result_summary`; delivery at **`g_deliver`** + **`work_items_export`**.
> Sections below retain v1 campaign-runner design for historical context; cross-check against v4 schema before coding.

Maintainer spec for long-running Computer tasks (days, 100–10,000 atomic units).
Builds on task board **v4**; does **not** replace milestone semantics.

Related:

- [`task-board-v2-schema.md`](task-board-v2-schema.md) — **authoritative v4 schema**
- [`task-board-parent-child-coordination.md`](task-board-parent-child-coordination.md)
- [`taskboard-lifecycle-and-fields.md`](taskboard-lifecycle-and-fields.md)

---

## 1. Problem statement

| Constraint | Pre-v4 | v4 target |
|------------|--------|-----------|
| Board rows in prompt | All tasks listed | `## Global milestones` + current row + conditional Item blocks |
| Atomic subtasks | Batched into 3–8 milestones | 100–10,000 in DB (`store_id`), not in board JSON |
| Session length | Single tool-loop run | Slice per run; resume next day |
| Evidence | `validate_results[]` on row | `remark` on row + `work_item.result_summary` |
| Scheduling | LLM-driven patch cadence | Host dequeue + `work_item_delta` / optional CampaignRunner |

**Non-goals (v1):**

- Parallel Computer UI automation (resource mutex deferred)
- Temporal / external workflow engine
- Replacing Supervisor team mode
- Auto-planner that bulk-seeds 10k **enumerated** rows without user/host/file input
- Using work_items for every task (see §3.5 — small jobs stay Type1 `global_milestones` only)

---

## 2. Architecture

Three layers; board holds **Wave** only.

```
Campaign (meta + stats + checkpoint)
  │
  ├─ work_items table     ← atomic units (1000+)
  │
  └─ global_milestones[] (Type1 or g_plan/g_exec/g_deliver)   ← task-level rows
         │
         ├─ item_milestones[] (Type2 SOP template)
         │
         └─ child boards (optional) ← local_* for one subtask
```

```text
┌─────────────────────────────────────────────────────────────┐
│  User / Cron / "继续"                                        │
└───────────────────────────┬─────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  CampaignRunner (host, session_inner or dedicated module)    │
│    load checkpoint → dequeue batch → run agent slice →       │
│    persist work_items + patch board stats → save checkpoint  │
└───────────────────────────┬─────────────────────────────────┘
                            ▼
        ┌───────────────────┴───────────────────┐
        ▼                                       ▼
 WorkItemStore (SQLite)                  TaskBoardStore (existing)
 work_items.db                           task_boards.db
```

**Design principles (aligned with Deep Agents / durable-agents):**

1. Board = cockpit; work queue = database.
2. LLM executes **one wave / few items**; host remembers cursor.
3. Prompt injects **working set** only.
4. Final delivery aggregates from `work_items` + board `remark` / `g_deliver`.

**Two campaign modes (§3.3):**

| Mode | Example | work_items created |
|------|---------|-------------------|
| `enumerated` | Open 1000 known apps | All at seed (host) |
| `dynamic` | Recruit: greet next 50 matches | JIT on `claim_work_slot` (host), target chosen by model |

---

## 3. Data model

### 3.1 Board document extensions (v3.1, backward compatible)

Add optional fields under `meta` and per-row `work_batch`:

```typescript
// TypeScript mirror for UI / API
interface CampaignMeta {
  // existing v3 fields ...
  campaign_id?: string           // stable id, default = store_key
  work_queue_enabled?: boolean   // true when campaign uses work_items
  campaign_mode?: 'enumerated' | 'dynamic'  // default enumerated when queue on
  selection_criteria?: string    // dynamic: filter prose for model (e.g. recruit rules)
  quota_total?: number           // dynamic: max units this campaign (stats.total)
  stats?: CampaignStats
  checkpoint?: CampaignCheckpoint
  run_budget?: CampaignRunBudget
}

interface CampaignStats {
  total: number
  pending: number
  ready: number
  in_progress: number
  done: number
  failed: number
  cancelled: number
  cursor_seq: number             // last assigned seq for dequeue
}

interface CampaignCheckpoint {
  version: number                // bump on schema change
  last_run_id: string
  last_run_ended_at_ms: number
  last_completed_item_id?: string
  active_wave_id?: string          // board row id currently in_progress
  stall_count: number              // replan trigger
}

interface CampaignRunBudget {
  max_items_per_run: number        // default 20
  max_tool_rounds_per_run: number  // reuse session max, cap slice
  max_wall_ms_per_run: number      // default 30 * 60 * 1000
}

interface BoardItem {
  // existing v3 fields ...
  work_batch?: {
    batch_id: string               // = row.id when 1:1
    item_id_from?: string          // inclusive work item id / seq
    item_id_to?: string
    item_count?: number
  }
}
```

Rust (`model.rs`):

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CampaignStats {
    pub total: u32,
    pub pending: u32,
    pub ready: u32,
    pub in_progress: u32,
    pub done: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub cursor_seq: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CampaignCheckpoint {
    pub version: u32,
    pub last_run_id: String,
    pub last_run_ended_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_completed_item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_wave_id: Option<String>,
    #[serde(default)]
    pub stall_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkBatchRef {
    pub batch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_count: Option<u32>,
}

// BoardMeta additions:
//   campaign_id, work_queue_enabled, campaign_mode, selection_criteria,
//   quota_total, stats, checkpoint, run_budget
// BoardItem additions:
//   work_batch: Option<WorkBatchRef>

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignMode {
    Enumerated,  // full list known at seed
    Dynamic,     // quota fixed; targets decided at runtime (claim)
}
```

**MetaStatus:** reuse existing `Paused` when run budget exhausted mid-campaign.

**`stats.total` semantics:**

- `enumerated`: count of seeded work_items (must match seed / `expected_total`).
- `dynamic`: `quota_total` — max successful units; work_items rows grow via `claim` until `done + failed >= quota` or user stops.

### 3.2 Work items table (new SQLite DB)

File: `{app_data}/work_items.db` (separate from `task_boards.db` to avoid document bloat).

```sql
CREATE TABLE IF NOT EXISTS work_items (
  id              TEXT PRIMARY KEY NOT NULL,  -- wi_{campaign_id}_{seq:06}
  campaign_id     TEXT NOT NULL,              -- = main task board store_key
  seq             INTEGER NOT NULL,
  batch_id        TEXT,                       -- maps to board row id
  status          TEXT NOT NULL,              -- pending|ready|in_progress|done|failed|cancelled
  title           TEXT NOT NULL,
  payload_json    TEXT NOT NULL DEFAULT '{}', -- target URL, app name, file path, etc.
  depends_on      TEXT NOT NULL DEFAULT '[]', -- JSON array of work item ids
  retry_count     INTEGER NOT NULL DEFAULT 0,
  max_retries     INTEGER NOT NULL DEFAULT 2,
  result_ref      TEXT,                       -- artifact path or inline summary
  result_json     TEXT,                       -- compact JSON for UI
  error_message   TEXT,
  created_at_ms   INTEGER NOT NULL,
  updated_at_ms   INTEGER NOT NULL,
  started_at_ms   INTEGER,
  finished_at_ms  INTEGER
);

CREATE INDEX IF NOT EXISTS idx_work_items_campaign_status
  ON work_items (campaign_id, status, seq);

CREATE INDEX IF NOT EXISTS idx_work_items_campaign_batch
  ON work_items (campaign_id, batch_id, seq);

CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_campaign_seq
  ON work_items (campaign_id, seq);

-- Dynamic mode dedupe: one greet / action per stable target key
CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_campaign_target_key
  ON work_items (campaign_id, json_extract(payload_json, '$.target_key'))
  WHERE json_extract(payload_json, '$.target_key') IS NOT NULL;
```

Payload schema (JSON, extensible):

```json
{
  "kind": "computer_ui",
  "target_label": "微信",
  "target_index": 3,
  "action_hint": "open and verify unread badge",
  "verify_criteria": "main window visible"
}
```

Dynamic mode payload (recruit greet example):

```json
{
  "kind": "recruit_greet",
  "target_key": "linkedin:ACoAAB…",
  "target_label": "张三 · 某公司",
  "profile_url": "https://…",
  "selection_notes": "matched: 3y Rust, open to work"
}
```

`target_key` is **required** for dynamic campaigns; host rejects `claim` when duplicate.

Artifact layout (workspace-relative):

```text
.pointer/campaigns/{campaign_id}/
  manifest.json           // total, batches, created_at
  items.jsonl             // optional bulk import mirror
  results/
    wi_..._000003.json    // per-item result
  batches/
    batch_02_summary.json // rolled-up evidence for board patch
```

### 3.3 Campaign modes: `enumerated` vs `dynamic`

#### `enumerated` (清单已知)

- **Use when:** user/file provides full target list; matrix/combinatorial with known `expected_total`.
- **Flow:** host `seed_batch` → all rows `pending|ready` → `dequeue_ready` → execute → `report_work_item`.
- **Board:** 3–8 wave rows mapping batch_id ranges (`batch_01` = items #1–#10).
- **Example:** open 1000 named apps from CSV.

#### `dynamic` (配额固定、对象运行时决定)

- **Use when:** action template repeats but **who** is chosen by model via search/filter (recruit auto-greet, “process next 50 matching rows in CRM”).
- **Flow:** host sets **quota only** — no upfront seed of N placeholder rows.
- **Per unit:**
  1. Model searches UI / reads list using `selection_criteria`.
  2. Model calls `task_board_claim_work_slot` with `target_key`, `target_label`, `payload`.
  3. Host creates one `work_item` (`in_progress`), checks quota + dedupe.
  4. Model executes action + `action_verify`.
  5. Model calls `task_board_report_work_item`.
- **Board:** often **1–2 milestones** (e.g. `execute_quota`, `deliver`); optional single row `progress: 12/50` without fixed batch ranges.
- **Wave advance:** skip or noop when only one logical row; campaign completes when `stats.done >= stats.total` (quota).

```text
【enumerated】  名单 → seed 1000 rows → dequeue #1 #2 …
【dynamic】    quota=50 + 条件 → 模型筛选 → claim 张三 → 打招呼 → report → claim 李四 → …
```

**Small dynamic jobs (≤50, single session):** may stay **v3 board-only** (one milestone, `progress N/M`, append `validate_results`) without work_items. Enable dynamic campaign when cross-day resume, per-person audit, or dedupe is required.

### 3.4 Who generates work_items

| Role | Responsibility |
|------|----------------|
| **User** | Goal, quota, criteria, optional JSONL/CSV for enumerated mode |
| **Model** | `seed_work_items` (short lists), `init_campaign_quota` (dynamic), **selects targets** in dynamic mode, `claim_work_slot`, execute, `report_work_item` |
| **Host** | **Always persists** rows: `seed_batch`, `claim_work_slot`, import JSONL; enforces quota, dedupe, stats; never guesses targets |
| **CampaignRunner** | Dequeue/ slice budget, checkpoint, pause — does **not** invent work_items |

**Generation paths:**

1. **Host file import** (enumerated, large): `campaign_import_jsonl(path)` → `seed_batch` — preferred for 200+ known rows.
2. **Model `task_board_seed_work_items`** (enumerated, small): inline `items[]`, host cap `MAX_INLINE_SEED` (default 200).
3. **Model `task_board_init_campaign_quota`** (dynamic): sets `quota_total`, `selection_criteria`, `campaign_mode=dynamic`; **zero** work_items until first `claim`.
4. **Model `task_board_claim_work_slot`** (dynamic): host inserts one row after quota/dedupe check.

Host **must not** allow unbounded append: dynamic campaigns only grow via `claim`; enumerated total fixed at seed unless explicit host `campaign_append_items` (v1.1).

### 3.5 When to use work_items (decision table)

| Situation | work_items? | Mode / API |
|-----------|-------------|------------|
| Single-step / narrow | No | skip board |
| Multi-step, ≤50 steps, one session | No | `task_board_init` only |
| Known list >50 or cross-day | Yes | `enumerated` + seed or import |
| Quota + model-filtered targets (recruit greet) | Yes (recommended if resume/dedupe) | `dynamic` + `init_campaign_quota` + `claim` |
| Quota ≤50, one session, no audit | Optional | v3 milestone `N/M` enough |

**Host routing (recommended):**

```rust
fn choose_campaign_path(user_text: &str, items_file: Option<&Path>, quota: Option<u32>) -> CampaignPath {
    if let Some(path) = items_file {
        return CampaignPath::EnumeratedImport(path);
    }
    if let Some(q) = quota.filter(|&n| n > 0) {
        if looks_like_dynamic_selection_task(user_text) {
            return CampaignPath::DynamicQuota(q);
        }
    }
    if inline_item_count <= 50 {
        CampaignPath::BoardOnly
    } else {
        CampaignPath::EnumeratedSeed
    }
}
```

### 3.6 Parent / child agents + work_items

Extends [`task-board-parent-child-coordination.md`](task-board-parent-child-coordination.md).
**v1 default:** single Computer lead owns the campaign queue. Multi-agent paths below are **v1.1** but the **data rules** apply as soon as sub-agents touch work_items.

#### 3.6.1 Principle (three layers)

```text
Parent board milestone     ← Supervisor task / batch / wave (task_2, batch_01)
    │
    ├─ work_items (parent campaign_id ONLY)   ← atomic units + dedupe + stats
    │
    └─ Child board local_*                    ← how to click/type/verify this unit
```

| Layer | Store key | Contents |
|-------|-----------|----------|
| Parent campaign | main-turn store key | `meta.stats`, work_items rows, parent `board[]` milestones |
| Child session | `{parent}\x1fptr_sub_agent\x1f{task_id}` | `local_*` rows only; **no** separate `campaign_id` |

**Single ledger rule:** `work_items.campaign_id` is always the **parent** store key. Sub-agents never get an isolated work_items partition (dedupe, quota, resume, and UI progress would diverge).

Today’s child `task_board` binding injects `_conversation_id` = **child store key** (`inject.rs`, `parent_child.rs`). Therefore sub-agents **must not** call parent-scoped `seed_work_items` / `report_work_item` / `claim_work_slot` directly on the parent campaign — same as they must not patch parent milestones. Use **host Gateway** (analogous to `report_child_status`).

#### 3.6.2 Collaboration patterns

**Pattern A — Single lead (v1 default)**

```text
User → Computer lead
         parent board + parent work_items
         claim / report / CampaignRunner @ parent
         no sub-agent queue split
```

**Pattern B — Supervisor: parent milestone = batch slice**

```text
Supervisor sync_parent_board:
  task_2 (computer) → assigned_batch_id: batch_01  (items #1–#10)

dispatch_to_child(task_2):
  child board seed local_01
  ChildAssignment { batch_id, sub_quota: 10 } stored on parent meta or dispatch ctx

Child computer run:
  [TASK_BOARD] local_*
  [TASK_BOARD_PARENT] goal + stats 12/50 + your_assignment batch_01
  per item: search → gateway.claim_child_work_slot → act → gateway.report_child_work_item
  end: report_child_status(task_2, done, "batch_01 10/10")
```

**Pattern C — Lead `run_subagent` one item**

```text
Parent lead: host assigns wi_…_042 to pending sub-run
run_subagent(computer, goal from item.payload)
Child: local steps only
Host on child exit: gateway.report_child_work_item(parent, wi_…_042, done)
Parent lead: continues dequeue / next item
```

#### 3.6.3 Assignment model

Optional columns on `work_items` (v1.1 migration):

```sql
ALTER TABLE work_items ADD COLUMN assigned_task_id TEXT;  -- parent milestone id (task_2)
ALTER TABLE work_items ADD COLUMN assigned_child_key TEXT; -- child store key while in_progress
```

Parent `BoardItem` extension for Supervisor dispatch:

```typescript
interface BoardItem {
  // existing ...
  work_assignment?: {
    batch_id?: string           // enumerated: all items with this batch_id
    item_id_from?: string
    item_id_to?: string
    sub_quota?: number          // dynamic: max claims for this milestone
    assigned_agent_id?: string  // e.g. computer
  }
}
```

**Enumerated:** child may only claim/dequeue items where `batch_id` matches `work_assignment.batch_id` (or id range).

**Dynamic:** child may `claim` until `claims_by_task[task_id] >= sub_quota`; global dedupe still on parent `target_key`.

#### 3.6.4 Gateway APIs (host-only, not LLM tools)

Location: `task_board/gateway/work_item_child.rs` (extends existing `gateway/`).

| API | Caller | Purpose |
|-----|--------|---------|
| `assign_work_slice` | `dispatch_to_child`, `run_subagent` | Bind `batch_id` / item range / `sub_quota` to `sub_task_id` |
| `claim_child_work_slot` | sub-agent tool handler (host) | Claim on **parent** campaign; checks assignment + dedupe + quota |
| `report_child_work_item` | sub-agent tool handler (host) | Update parent item status; refresh parent `meta.stats` |
| `list_assigned_work_items` | inject / UI | Page items for child’s assignment (working set) |
| `release_child_assignments` | after `report_child_status` failed/cancel | Reset `in_progress` items assigned to child |

```rust
pub struct ChildWorkAssignment {
    pub parent_store_key: String,
    pub sub_task_id: String,
    pub child_store_key: String,
    pub batch_id: Option<String>,
    pub item_id_from: Option<String>,
    pub item_id_to: Option<String>,
    pub sub_quota: Option<u32>,
}

pub fn claim_child_work_slot(
    work_item_store: &WorkItemStore,
    parent_key: &str,
    assignment: &ChildWorkAssignment,
    claim: WorkItemClaim,
) -> Result<ClaimOutcome> {
    assert_assignment_allows_claim(assignment, &claim)?;
    work_item_store.claim_work_slot(parent_key, /* parent doc */, claim)
}

pub fn report_child_work_item(
    work_item_store: &WorkItemStore,
    task_board_store: &TaskBoardStore,
    parent_key: &str,
    sub_task_id: &str,
    item_id: &str,
    report: WorkItemReport,
) -> Result<()> {
    let outcome = work_item_store.report_item(parent_key, item_id, report)?;
    sync_parent_stats_and_progress(task_board_store, parent_key, sub_task_id)?;
    // does NOT patch parent milestone status (still report_child_status at end)
    Ok(())
}
```

#### 3.6.5 Child-facing tools (scoped sidecar)

Register **child-only** tool names that always write the **parent** campaign via Gateway (child `_conversation_id` remains child store key; host adds `_parent_campaign_id` + `_sub_task_id`):

| Tool | When | Host action |
|------|------|-------------|
| `task_board_claim_work_slot` | dynamic, child run | `claim_child_work_slot` if `assignment` present; else error on child key |
| `task_board_report_work_item` | after each unit | `report_child_work_item` |
| `task_board_patch` | local row only | unchanged; child store only |

Parent lead keeps unscoped `seed_work_items`, `init_campaign_quota`, parent `patch` on milestones.

#### 3.6.6 Prompt injection (`[TASK_BOARD_PARENT]` extension)

When child runs under a campaign, append to read-only parent tunnel (`format_parent_tunnel_block` / `sub_agent_prompt.rs`):

```text
[TASK_BOARD_PARENT]
read_only: true
goal: …
campaign_progress: 12/50 done, 1 failed
your_assignment: batch_01 items #1–#10 (sub_quota 10)
selection_criteria: …          # dynamic only
current_item: wi_…_003 张三   # if host pre-dequeued one item for this slice
findings: …
```

Do **not** inject the full work_items list — at most **assigned slice summary** + optional **single current item** (working set).

#### 3.6.7 Dynamic recruit + multiple computer children

```text
Parent: init_campaign_quota(50, "3y Rust…")
Supervisor parent board:
  task_c1: computer, sub_quota 10
  task_c2: computer, sub_quota 10, depends_on task_c1
  … (5 children × 10)

Each child:
  model picks candidates → claim_child_work_slot (parent dedupe on target_key)
  greet → report_child_work_item
  stops at sub_quota 10 or budget

Parent stats.done → 50 ⇒ campaign complete.
Same target_key cannot be claimed by two children (parent unique index).
```

#### 3.6.8 ACL summary

| Action | Parent lead | Child agent |
|--------|-------------|-------------|
| `seed_work_items` / `init_campaign_quota` | ✅ parent store | ❌ |
| Parent milestone `patch` | ✅ | ❌ |
| Child `local_*` patch | N/A | ✅ |
| Any write to the parent board | ✅ | ❌ |
| `claim` / `report` on parent campaign | ✅ (direct) | ✅ **via Gateway only** |
| `report_child_status` | — | host @ sub-agent exit |

#### 3.6.9 Rollout

| Phase | Scope |
|-------|--------|
| v1 | Single lead + work_items; existing child boards without queue |
| v1.1 | Gateway + child scoped tools + `[TASK_BOARD_PARENT]` assignment fields |
| v1.2 | Supervisor `work_assignment` on planned `AgentTask`; parallel computer children with mutex |

---

## 4. Module layout (Rust)

```text
crates/pointer-core/src/task_board/
  campaign/
    mod.rs              // public API
    runner.rs           // CampaignRunner
    stats.rs            // recompute stats from work_items
    wave.rs             // open/close wave, sync board rows
    replan.rs           // stall detection, host-only replace
  work_item/
    mod.rs
    model.rs
    store.rs            // WorkItemStore (memory + sqlite)
    persistence/sqlite.rs
    seed.rs             // import jsonl / batch create
    claim.rs            // dynamic: claim_work_slot + dedupe
    artifact.rs         // read/write result files
  gateway/
    work_item_child.rs  // claim/report child → parent campaign (§3.6)
  snapshot_working_set.rs  // inject mode for large campaigns
```

Register in `task_board/mod.rs`:

```rust
pub mod campaign;
pub mod work_item;
pub use campaign::CampaignRunner;
pub use work_item::WorkItemStore;
```

AppState (`app_state.rs`):

```rust
pub struct AppState {
    // existing ...
    pub task_board_store: Arc<TaskBoardStore>,
    pub work_item_store: Arc<WorkItemStore>,  // NEW
}
```

Open persistence (mirror `open_default_persistence`):

```rust
pub fn open_default_work_item_persistence() -> Option<Arc<WorkItemSqlite>> {
    let dir = crate::storage::app_data_dir().ok()?;
    WorkItemSqlite::open(dir.join("work_items.db")).ok()
}
```

---

## 5. WorkItemStore — pseudocode

```rust
pub struct WorkItemStore {
    inner: RwLock<HashMap<String, HashMap<String, WorkItem>>>, // campaign_id -> id -> item
    persistence: RwLock<Option<Arc<WorkItemSqlite>>>,
}

impl WorkItemStore {
    pub fn seed_batch(
        &self,
        campaign_id: &str,
        items: Vec<WorkItemDraft>,
        batch_size: usize,  // default 10
    ) -> Result<SeedOutcome> {
        // Pseudocode:
        let mut seq = self.max_seq(campaign_id)? + 1;
        let mut batch_id = String::new();
        let mut batches: Vec<(String, u32, u32)> = vec![]; // batch_id, from_seq, to_seq
        let mut count_in_batch = 0u32;

        for draft in items {
            if count_in_batch == 0 {
                batch_id = format!("batch_{:02}", batches.len() + 1);
                batches.push((batch_id.clone(), seq, seq)); // to_seq updated below
            }
            let id = format!("wi_{campaign_id}_{seq:06}");
            let item = WorkItem {
                id,
                campaign_id: campaign_id.into(),
                seq,
                batch_id: Some(batch_id.clone()),
                status: WorkItemStatus::Pending,
                title: draft.title,
                payload_json: draft.payload,
                depends_on: draft.depends_on,
                max_retries: draft.max_retries.unwrap_or(2),
                ..Default::default()
            };
            self.insert_persist(&item)?;
            seq += 1;
            count_in_batch += 1;
            if count_in_batch >= batch_size as u32 {
                batches.last_mut().unwrap().2 = seq - 1;
                count_in_batch = 0;
            }
        }
        if count_in_batch > 0 {
            batches.last_mut().unwrap().2 = seq - 1;
        }

        Ok(SeedOutcome { total: seq - 1, batches })
    }

    pub fn dequeue_ready(
        &self,
        campaign_id: &str,
        limit: usize,
        active_batch_id: Option<&str>,
    ) -> Result<Vec<WorkItem>> {
        // Pseudocode:
        // 1. SELECT ... WHERE campaign_id AND status IN ('ready','pending')
        //    AND (batch_id = active_batch OR active_batch is None)
        //    ORDER BY seq ASC LIMIT limit
        // 2. For each: CAS status pending|ready -> in_progress (optimistic lock)
        // 3. Return claimed items; failed CAS skipped
        todo!()
    }

    pub fn report_item(
        &self,
        campaign_id: &str,
        item_id: &str,
        report: WorkItemReport,
    ) -> Result<WorkItemReportOutcome> {
        // Pseudocode:
        let mut item = self.get(campaign_id, item_id)?;
        match report.status {
            Done => {
                item.status = WorkItemStatus::Done;
                item.result_ref = Some(write_artifact(campaign_id, &item, &report)?);
                item.result_json = Some(compact_json(&report.summary));
                item.finished_at_ms = now_ms();
            }
            Failed => {
                item.retry_count += 1;
                if item.retry_count >= item.max_retries {
                    item.status = WorkItemStatus::Failed;
                    item.error_message = Some(report.error);
                    item.finished_at_ms = now_ms();
                } else {
                    item.status = WorkItemStatus::Ready; // retry
                }
            }
        }
        self.update_persist(&item)?;
        Ok(WorkItemReportOutcome {
            item,
            batch_complete: self.is_batch_terminal(campaign_id, item.batch_id.as_deref()?),
        })
    }

    pub fn recompute_stats(&self, campaign_id: &str) -> Result<CampaignStats> {
        // Single SQL GROUP BY status or in-memory fold
        todo!()
    }

    /// Dynamic mode: create one work_item when model commits to a target.
    pub fn claim_work_slot(
        &self,
        campaign_id: &str,
        doc: &BoardDocument,
        claim: WorkItemClaim,
    ) -> Result<ClaimOutcome> {
        let mode = doc.meta.campaign_mode.unwrap_or(CampaignMode::Enumerated);
        if mode != CampaignMode::Dynamic {
            return Err(anyhow!("claim_work_slot only in campaign_mode=dynamic"));
        }
        let quota = doc.meta.quota_total.or(doc.meta.expected_total)
            .ok_or_else(|| anyhow!("dynamic campaign missing quota_total"))?;
        let stats = self.recompute_stats(campaign_id)?;
        let active = stats.in_progress;
        let terminal = stats.done + stats.failed + stats.cancelled;
        if terminal + active >= quota {
            return Err(anyhow!("campaign quota exhausted ({quota})"));
        }
        let target_key = claim.target_key.trim();
        if target_key.is_empty() {
            return Err(anyhow!("claim requires payload.target_key"));
        }
        if self.exists_target_key(campaign_id, target_key)? {
            return Err(anyhow!(
                "duplicate target_key; choose another candidate",
                target_key = target_key,
            ));
        }

        let seq = self.next_seq(campaign_id)?;
        let id = format!("wi_{campaign_id}_{seq:06}");
        let item = WorkItem {
            id: id.clone(),
            campaign_id: campaign_id.into(),
            seq,
            batch_id: doc.meta.checkpoint.as_ref()
                .and_then(|c| c.active_wave_id.clone()),
            status: WorkItemStatus::InProgress,
            title: claim.target_label,
            payload_json: claim.payload_with_target_key(target_key),
            started_at_ms: Some(now_ms()),
            ..Default::default()
        };
        self.insert_persist(&item)?;
        Ok(ClaimOutcome {
            item_id: id,
            seq,
            quota_total: quota,
            done: stats.done,
            in_progress: stats.in_progress + 1,
        })
    }

    pub fn init_dynamic_campaign(
        &self,
        store_key: &str,
        doc: &mut BoardDocument,
        quota: u32,
        selection_criteria: &str,
        goal: &str,
    ) -> Result<()> {
        doc.meta.work_queue_enabled = Some(true);
        doc.meta.campaign_mode = Some(CampaignMode::Dynamic);
        doc.meta.quota_total = Some(quota);
        doc.meta.expected_total = Some(quota);
        doc.meta.selection_criteria = Some(selection_criteria.to_string());
        doc.meta.goal = goal.to_string();
        doc.meta.stats = Some(CampaignStats {
            total: quota,
            ..Default::default()
        });
        doc.meta.checkpoint = Some(CampaignCheckpoint { version: 1, ..Default::default() });
        doc.meta.run_budget = Some(default_run_budget());
        // Single board row for quota tracking (no batch ranges)
        doc.board = vec![BoardItem {
            id: "quota_exec".into(),
            title: format!("Execute quota ({quota} units)"),
            status: ItemStatus::InProgress,
            validate_requirement: Some(selection_criteria.to_string()),
            progress: Some(format!("0/{quota}")),
            ..Default::default()
        }];
        doc.meta.checkpoint.as_mut().unwrap().active_wave_id = Some("quota_exec".into());
        Ok(())
    }
}
```

---

## 6. New tools (LLM-facing)

Host-only tools are **not** exposed to the model. New sidecar tools follow flat `task_board_*` naming.

### 6.1 `task_board_seed_work_items` (sidecar, `enumerated`)

Seeds queue + opens first wave on board. Replaces manual 1000-row `init`.
Sets `campaign_mode = enumerated`.

```yaml
# task_board.schema.yaml addition
task_board_seed_work_items:
  properties:
    items:
      type: array
      items:
        type: object
        required: [title]
        properties:
          title: { type: string }
          payload: { type: object }
          depends_on: { type: array, items: { type: string } }
    batch_size: { type: integer, minimum: 1, maximum: 50, default: 10 }
    goal: { type: string }
    expected_total: { type: integer }
```

Handler pseudocode:

```rust
fn apply_seed_work_items(store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    let items = parse_item_drafts(args)?;
    let batch_size = args.get("batch_size").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    // 1. Seed work_items
    let seed = work_item_store.seed_batch(store_key, items, batch_size)?;
    write_manifest_artifact(store_key, &seed)?;

    // 2. Update meta
    doc.meta.work_queue_enabled = Some(true);
    doc.meta.campaign_mode = Some(CampaignMode::Enumerated);
    doc.meta.campaign_id = Some(store_key.to_string());
    doc.meta.expected_total = Some(seed.total);
    doc.meta.stats = Some(work_item_store.recompute_stats(store_key)?);
    doc.meta.checkpoint = Some(CampaignCheckpoint {
        version: 1,
        ..Default::default()
    });
    doc.meta.run_budget = Some(default_run_budget());
    if let Some(goal) = goal_from_args(args) {
        doc.meta.goal = goal;
    }

    // 3. Materialize FIRST WAVE only onto board (not all batches)
    doc.board = wave::materialize_wave(doc, &seed.batches, wave_index: 0)?;

    mark_ready_pending_rows(doc);
    Ok(json!({
        "ok": true,
        "method": "seed_work_items",
        "total": seed.total,
        "batch_count": seed.batches.len(),
        "board_len": doc.board.len(),
    }))
}
```

}
```

### 6.2 `task_board_init_campaign_quota` (sidecar, `dynamic`)

Starts a quota campaign **without** pre-seeding work_items.  
Example: “Greet 50 candidates matching JD filters.”

```yaml
task_board_init_campaign_quota:
  properties:
    goal: { type: string }
    quota_total: { type: integer, minimum: 1, maximum: 10000 }
    selection_criteria: { type: string }  # markdown: filters, exclusions
```

Handler pseudocode:

```rust
fn apply_init_campaign_quota(store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    let quota = require_u32(args, "quota_total")?;
    let criteria = require_str(args, "selection_criteria")?;
    let goal = goal_from_args(args).unwrap_or_else(|| "Dynamic quota campaign".into());
    work_item_store.init_dynamic_campaign(store_key, doc, quota, &criteria, &goal)?;
    task_board_store.save_document(store_key, doc.clone());
    Ok(json!({
        "ok": true,
        "method": "init_campaign_quota",
        "campaign_mode": "dynamic",
        "quota_total": quota,
        "board_len": doc.board.len(),
    }))
}
```

### 6.3 `task_board_claim_work_slot` (sidecar, `dynamic`)

Model calls **after** choosing the next target, **before** UI action.

```yaml
task_board_claim_work_slot:
  properties:
    target_key: { type: string }    # stable id for dedupe (profile id, row id)
    target_label: { type: string }  # display name for board/artifact
    payload: { type: object }       # optional extra fields
```

Handler pseudocode:

```rust
fn apply_claim_work_slot(store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    let claim = WorkItemClaim::from_args(args)?;
    let outcome = work_item_store.claim_work_slot(store_key, doc, claim)?;
    doc.meta.stats = Some(work_item_store.recompute_stats(store_key)?);
    if let Some(row) = doc.board.iter_mut().find(|r| r.id == "quota_exec") {
        let s = doc.meta.stats.as_ref().unwrap();
        row.progress = Some(format!("{}/{}", s.done + s.in_progress, s.total));
    }
    task_board_store.save_document(store_key, doc.clone());
    Ok(json!({
        "ok": true,
        "method": "claim_work_slot",
        "item_id": outcome.item_id,
        "seq": outcome.seq,
        "quota": format!("{}/{}", outcome.done + outcome.in_progress, outcome.quota_total),
    }))
}
```

Errors (tool result, model must pick another target):

- `quota_exhausted`
- `duplicate_target_key`
- `wrong_campaign_mode`

### 6.4 `task_board_report_work_item` (sidecar)

Called by Computer each time one atomic unit finishes (same turn as `action_verify` + patch).

```rust
fn apply_report_work_item(store_key: &str, doc: &mut BoardDocument, args: &Value) -> Result<Value> {
    let item_id = require_str(args, "item_id")?;
    let status = parse_item_status(args.get("status"))?;
    let summary = optional_str(args, "summary")?;
    let error = optional_str(args, "error")?;

    let outcome = work_item_store.report_item(store_key, item_id, WorkItemReport {
        status, summary, error, ..Default::default()
    })?;

    // Sync stats to meta (host authoritative)
    doc.meta.stats = Some(work_item_store.recompute_stats(store_key)?);

    // Append ONE line to active wave row validate_results (not full item dump)
    if let Some(wave_id) = doc.meta.checkpoint.as_mut().and_then(|c| c.active_wave_id.clone()) {
        if let Some(row) = doc.board.iter_mut().find(|r| r.id == wave_id) {
            let line = format!("#{} {}: {}", outcome.item.seq, outcome.item.title, summary.unwrap_or(""));
            push_snippet(&mut row.validate_results, &line);
            let s = doc.meta.stats.as_ref();
            if doc.meta.campaign_mode == Some(CampaignMode::Dynamic) {
                if let Some(st) = s {
                    row.progress = Some(format!("{}/{}", st.done, st.total));
                }
            } else if let Some(b) = row.work_batch.as_ref() {
                row.progress = Some(format!(
                    "{}/{}",
                    batch_done_count(store_key, row)?,
                    b.item_count.unwrap_or(0)
                ));
            }
        }
    }

    // Dynamic: campaign complete when done >= quota
    if doc.meta.campaign_mode == Some(CampaignMode::Dynamic) {
        if let Some(st) = doc.meta.stats.as_ref() {
            if st.done >= st.total && st.total > 0 {
                doc.meta.status = MetaStatus::Completed;
            }
        }
    }

    Ok(json!({
        "ok": true,
        "method": "report_work_item",
        "item_id": item_id,
        "item_status": outcome.item.status.as_str(),
        "batch_complete": outcome.batch_complete,
        "stats": doc.meta.stats,
    }))
}
```

### 6.5 Host-only APIs (no LLM tool)

```rust
// campaign/wave.rs
pub fn advance_wave_if_complete(
    task_board_store: &TaskBoardStore,
    work_item_store: &WorkItemStore,
    store_key: &str,
) -> Result<Option<WaveAdvanceOutcome>> {
    let mut doc = task_board_store.document(store_key);
    let Some(active_id) = doc.meta.checkpoint.as_ref().and_then(|c| c.active_wave_id.clone()) else {
        return Ok(None);
    };
    let row = doc.board.iter().find(|r| r.id == active_id).context("wave row missing")?;
    let batch_id = row.work_batch.as_ref().map(|b| b.batch_id.clone()).context("no work_batch")?;

    if !work_item_store.is_batch_terminal(store_key, &batch_id)? {
        return Ok(None);
    }

    // Mark wave row done (host patch, bypass LLM)
    apply_host_patch_row_done(&mut doc, &active_id, rollup_batch_summary(store_key, &batch_id)?)?;

    let next_wave = wave::next_wave_index(&doc);
    if next_wave >= wave::wave_count(store_key)? {
        doc.meta.status = MetaStatus::Completed;
        return Ok(Some(WaveAdvanceOutcome::CampaignComplete));
    }

    // Replace board with next wave rows ONLY (prune completed from inject)
    doc.board = wave::materialize_wave(&doc, wave::all_batches(store_key)?, next_wave)?;
    doc.meta.checkpoint.as_mut().unwrap().active_wave_id =
        doc.board.iter().find(|r| r.status == ItemStatus::InProgress)
            .or_else(|| doc.board.first())
            .map(|r| r.id.clone());
    mark_ready_pending_rows(&mut doc);
    task_board_store.save_document(store_key, doc);
    Ok(Some(WaveAdvanceOutcome::Advanced { wave_index: next_wave }))
}

// gateway/work_item_child.rs — §3.6
pub fn claim_child_work_slot(/* … */) -> Result<ClaimOutcome> { todo!() }
pub fn report_child_work_item(/* … */) -> Result<()> { todo!() }
pub fn assign_work_slice(/* … */) -> Result<ChildWorkAssignment> { todo!() }
```

### 6.6 Child-scoped work_item tools (sub-agent only)

When `is_child_store_key(_conversation_id)` and parent has `work_queue_enabled`, register the same tool **names** but handlers route through Gateway with host-injected:

- `_parent_campaign_id` — parent main-turn store key  
- `_sub_task_id` — parent milestone id (`task_2`, …)  
- `_assignment` — resolved `ChildWorkAssignment` from `dispatch_to_child`

Child lead **without** assignment gets tool result error `no_work_assignment` (fall back to local board only).

See §3.6.4–3.6.5 for API table and ACL.

---

## 7. CampaignRunner — host loop pseudocode

Entry: end of `run_chat_inner` when campaign active, **or** dedicated pre-loop hook before tool rounds.

```rust
pub struct CampaignRunner {
    task_board_store: Arc<TaskBoardStore>,
    work_item_store: Arc<WorkItemStore>,
}

pub struct RunSliceOutcome {
    pub items_completed: u32,
    pub items_failed: u32,
    pub budget_exhausted: bool,
    pub campaign_complete: bool,
}

impl CampaignRunner {
    /// Called once per user message / resume when meta.work_queue_enabled && status == Running|Paused
    pub async fn maybe_run_slice(
        &self,
        ctx: &mut ChatRunContext<'_>,
        store_key: &str,
    ) -> Result<Option<RunSliceOutcome>> {
        let doc = self.task_board_store.document(store_key);
        if !doc.meta.work_queue_enabled.unwrap_or(false) {
            return Ok(None);
        }
        if matches!(doc.meta.status, MetaStatus::Completed | MetaStatus::Failed) {
            return Ok(None);
        }

        let budget = doc.meta.run_budget.clone().unwrap_or_default();
        let run_id = ctx.run_id.clone();
        let started = Instant::now();
        let mut completed = 0u32;
        let mut failed = 0u32;

        // Ensure active wave row exists
        self.ensure_active_wave(store_key)?;

        let mode = doc.meta.campaign_mode.unwrap_or(CampaignMode::Enumerated);

        loop {
            if completed + failed >= budget.max_items_per_run {
                break;
            }
            if started.elapsed().as_millis() as u64 >= budget.max_wall_ms_per_run {
                break;
            }
            if ctx.tool_budget.remaining() == 0 {
                break;
            }
            if ctx.cancel.is_cancelled() {
                break;
            }

            // --- enumerated: host dequeues pre-seeded item ---
            // --- dynamic: no dequeue; model claim → act → report inside agent loop ---
            if mode == CampaignMode::Dynamic {
                // Inject quota + criteria; one agent slice may complete multiple claim/report cycles
                inject_campaign_quota_focus(ctx, store_key, &doc);
                let slice_outcome = self.run_dynamic_quota_slice(ctx, store_key, &budget).await?;
                completed += slice_outcome.completed;
                failed += slice_outcome.failed;
                if slice_outcome.quota_complete {
                    return Ok(Some(RunSliceOutcome {
                        items_completed: completed,
                        items_failed: failed,
                        budget_exhausted: false,
                        campaign_complete: true,
                    }));
                }
                break; // budget handled inside run_dynamic_quota_slice
            }

            // Dequeue one item (Computer: serial UI)
            let batch_id = self.active_batch_id(store_key)?;
            let mut items = self.work_item_store.dequeue_ready(
                store_key,
                limit: 1,
                Some(&batch_id),
            )?;
            if items.is_empty() {
                // Try advance wave
                if let Some(out) = advance_wave_if_complete(
                    &self.task_board_store,
                    &self.work_item_store,
                    store_key,
                )? {
                    if matches!(out, WaveAdvanceOutcome::CampaignComplete) {
                        return Ok(Some(RunSliceOutcome {
                            items_completed: completed,
                            items_failed: failed,
                            budget_exhausted: false,
                            campaign_complete: true,
                        }));
                    }
                    continue;
                }
                break; // nothing ready, wait for user
            }

            let item = items.remove(0);

            // Inject item context for this slice (user dynamic block, not full queue)
            inject_work_item_focus(ctx, &item);

            // Run agent until item terminal OR one tool-round budget consumed
            let item_outcome = self.run_item_slice(ctx, store_key, &item).await?;

            match item_outcome {
                ItemSliceOutcome::Done { summary } => {
                    self.apply_report_work_item_host(store_key, &item.id, Done, summary)?;
                    completed += 1;
                }
                ItemSliceOutcome::Failed { error } => {
                    self.apply_report_work_item_host(store_key, &item.id, Failed, error)?;
                    failed += 1;
                }
                ItemSliceOutcome::Deferred => break,
            }

            // History trim after item (reuse task_board trim)
            maybe_trim_after_tool_pass(ctx, store_key);
        }

        self.save_checkpoint(store_key, &run_id, completed, failed)?;

        let doc = self.task_board_store.document(store_key);
        let total_done = doc.meta.stats.as_ref().map(|s| s.done).unwrap_or(0);
        let total = doc.meta.stats.as_ref().map(|s| s.total).unwrap_or(0);
        let campaign_complete = total > 0 && total_done >= total;

        Ok(Some(RunSliceOutcome {
            items_completed: completed,
            items_failed: failed,
            budget_exhausted: completed + failed >= budget.max_items_per_run,
            campaign_complete,
        }))
    }

    async fn run_item_slice(
        &self,
        ctx: &mut ChatRunContext<'_>,
        store_key: &str,
        item: &WorkItem,
    ) -> Result<ItemSliceOutcome> {
        // Pseudocode: narrow agent loop
        //
        // system += "[WORK_ITEM_FOCUS]\nitem_id: ...\ntitle: ...\npayload: ...\n"
        // user   += "Complete this single work item. When done, call
        //           task_board_report_work_item + action_verify in same turn."
        //
        // loop with sub-budget (e.g. max 15 tool rounds per item):
        //   stream_chat_once()
        //   if host detects report_work_item success for item.id:
        //       return Done
        //   if unrecoverable error:
        //       return Failed
        //
        // return Deferred if budget hit mid-item
        todo!()
    }

    /// Dynamic quota campaigns: reuse main agent tool loop; count report_work_item successes.
    async fn run_dynamic_quota_slice(
        &self,
        ctx: &mut ChatRunContext<'_>,
        store_key: &str,
        budget: &CampaignRunBudget,
    ) -> Result<DynamicSliceOutcome> {
        // Pseudocode:
        // inject [CAMPAIGN_QUOTA] with selection_criteria + stats.done/stats.total
        // delegate to existing single-agent loop until:
        //   - report_work_item count >= max_items_per_run, OR
        //   - wall clock / tool budget, OR
        //   - stats.done >= stats.total
        // return DynamicSliceOutcome { completed, failed, quota_complete }
        todo!()
    }
}
```

**Dynamic mode — model turn cadence (e.g. recruit greet):**

```text
search/filter in UI
  → task_board_claim_work_slot { target_key, target_label }
  → greet + action_verify
  → task_board_report_work_item { item_id, status: done, summary }
(repeat until quota or run budget)
```

Host does **not** select targets; it enforces quota, dedupe, checkpoint, and pause/resume.

**Integration point** (`session_inner.rs`):

```rust
async fn run_chat_inner(ctx: &mut ChatRunContext<'_>, req: &ChatRunRequest<'_>) -> Result<()> {
    // ... existing setup ...
    let store_key = choose_main_task_board_store_key(&state, conversation_id, history);

    // NEW: before main agent loop
    if let Some(slice) = state.campaign_runner.maybe_run_slice(ctx, &store_key).await? {
        emit_campaign_progress(&stream, conversation_id, &slice);
        if slice.campaign_complete {
            // trigger finalize hint inject
        } else if slice.budget_exhausted {
            // set meta.status = Paused; UI shows "继续执行"
            pause_campaign(&state.task_board_store, &store_key)?;
        }
    }

    // ... existing single-agent / supervisor loop ...
}
```

---

## 8. Working set snapshot — pseudocode

New file: `snapshot_working_set.rs`. Selected when `meta.work_queue_enabled == true`.

```rust
const WAVE_ROW_CAP: usize = 8;
const READY_HINT_COUNT: usize = 2;
const DONE_SUMMARY_CAP: usize = 3;

pub fn markdown_working_set_block(
    store_key: &str,
    doc: &BoardDocument,
    stats: &CampaignStats,
) -> String {
    let mut lines = vec!["[TASK_BOARD]".into(), "inject_mode: working_set".into(), String::new()];

    // Global
    lines.push("## Campaign".into());
    lines.push(format!("- goal: {}", doc.meta.goal.trim()));
    lines.push(format!(
        "- progress: {}/{} done, {} failed, {} in_progress",
        stats.done, stats.total, stats.failed, stats.in_progress
    ));
    if doc.meta.campaign_mode == Some(CampaignMode::Dynamic) {
        if let Some(c) = doc.meta.selection_criteria.as_deref() {
            lines.push(format!("- selection_criteria: {}", truncate_field(Some(c), 400)));
        }
    }
    if let Some(wid) = doc.meta.checkpoint.as_ref().and_then(|c| c.active_wave_id.as_deref()) {
        lines.push(format!("- active_wave: {wid}"));
    }

    // Recent findings (unchanged tail)
    for f in doc.global_context.key_findings.iter().rev().take(8) {
        lines.push(format!("- finding: {f}"));
    }

    lines.push(String::new());
    lines.push("## Active wave (board)".into());
    for item in doc.board.iter().take(WAVE_ROW_CAP) {
        lines.push(format_all_tasks_line(item)); // existing helper
    }
    if doc.board.len() > WAVE_ROW_CAP {
        lines.push(format!("- … {} more wave rows omitted", doc.board.len() - WAVE_ROW_CAP));
    }

    // Completed waves: one-line summary each, not full validate_results
    lines.push(String::new());
    lines.push("## Completed waves (summary)".into());
    for summary in load_done_wave_summaries(store_key, DONE_SUMMARY_CAP) {
        lines.push(format!("- {summary}"));
    }
    if stats.done > 0 {
        lines.push(format!(
            "- … {} atomic items done; details in artifacts, not injected",
            stats.done
        ));
    }

    // Current task detail (same as v3 current task section)
    lines.push(String::new());
    append_current_task_sections(&mut lines, doc); // reuse snapshot.rs

    lines.join("\n")
}

pub fn snapshot_for_prompt(store_key: &str, doc: &BoardDocument, compact: bool) -> Option<String> {
    if doc.meta.work_queue_enabled.unwrap_or(false) {
        let stats = doc.meta.stats.clone().unwrap_or_default();
        return Some(markdown_working_set_block(store_key, doc, &stats));
    }
    // fall through to existing v3 snapshot
    existing_snapshot_for_prompt(store_key, doc, compact)
}
```

**Token budget target:** working set block ≤ 4K tokens (enforced by caps above + existing `PLAN_INJECT_MAX`).

---

## 9. Wave materialization — pseudocode

```rust
pub fn materialize_wave(
    doc: &BoardDocument,
    batches: &[(String, u64, u64)], // batch_id, from_seq, to_seq
    wave_index: usize,
    waves_per_board: usize,       // default 1 batch == 1 row
) -> Result<Vec<BoardItem>> {
    let start = wave_index * waves_per_board;
    let slice = &batches[start..batches.len().min(start + waves_per_board)];

    let mut board = Vec::new();
    for (batch_id, from_seq, to_seq) in slice {
        let count = (to_seq - from_seq + 1) as u32;
        let title = format!("Batch {} (items #{}–#{})", batch_id, from_seq, to_seq);
        board.push(BoardItem {
            id: batch_id.clone(),
            title,
            status: if board.is_empty() { ItemStatus::InProgress } else { ItemStatus::Pending },
            validate_requirement: Some(format!(
                "Complete all {} items in this batch with evidence per item",
                count
            )),
            work_batch: Some(WorkBatchRef {
                batch_id: batch_id.clone(),
                item_id_from: Some(format!("wi_{}_{:06}", doc.task_id, from_seq)),
                item_id_to: Some(format!("wi_{}_{:06}", doc.task_id, to_seq)),
                item_count: Some(count),
            }),
            ..Default::default()
        });
    }
    Ok(board)
}
```

---

## 10. Resume and continuation

Reuse `looks_like_resume_intent` + extend:

```rust
fn should_auto_resume_campaign(doc: &BoardDocument) -> bool {
    doc.meta.work_queue_enabled == Some(true)
        && matches!(doc.meta.status, MetaStatus::Running | MetaStatus::Paused)
        && doc.meta.stats.as_ref().is_some_and(|s| s.done < s.total)
}

fn on_user_message(store_key: &str, user_content: &str, doc: &BoardDocument) -> MetaStatus {
    if should_auto_resume_campaign(doc) {
        if looks_like_resume_intent(user_content) || looks_like_implicit_continue(user_content) {
            MetaStatus::Running // clear Paused
        } else {
            doc.meta.status // unchanged
        }
    } else {
        doc.meta.status
    }
}
```

UI (APP + WEB): unfinished campaign shows progress bar + **Continue** button → sends fixed resume phrase (i18n, not dev text in label).

---

## 11. Guardrails

| Guardrail | v3 today | Campaign v1 |
|-----------|----------|-------------|
| Tool rounds | per user message (settings) | same |
| `expected_total` | init/replace row count | Must match `stats.total` after seed |
| Board row count | prompt says 3–12 | Hard cap 15 on board; seed rejects |
| Item retries | row `retry_count` | `work_items.retry_count` / `max_retries` |
| Stall replan | manual replace | `stall_count >= 3` → host `replan.rs` |

Board **`max_steps` / `step_count`** removed — patch volume is bounded by **tool rounds** only.
One **`task_board_patch`** = **one atomic work item** (see runtime prompt Mode A / B).

Campaign **`MetaStatus::Paused`** remains for future run-budget / resume UX (not step-count).

---

## 12. Observability

Log prefix: `task_board_obs:` (existing) + `campaign_obs:`.

```rust
// campaign/observability.rs
log::info!(
    "campaign_obs: slice_end store_key={} run_id={} completed={} failed={} budget_exhausted={} stats_done={}/{}",
    store_key, run_id, completed, failed, budget_exhausted, stats.done, stats.total
);
log::info!(
    "campaign_obs: wave_advanced store_key={} wave_index={} batch_id={}",
    store_key, wave_index, batch_id
);
log::info!(
    "campaign_obs: item_terminal store_key={} item_id={} status={} retry_count={}",
    store_key, item_id, status, retry_count
);
```

Stream events (APP + WEB):

```typescript
interface StreamEventCampaignProgress {
  type: 'campaign_progress'
  conversation_id: string
  store_key: string
  stats: CampaignStats
  slice?: { completed: number; failed: number; budget_exhausted: boolean }
}
```

---

## 13. UI (cross-entry)

`TaskBoardPanel` extensions:

```typescript
function CampaignProgressBar({ stats }: { stats: CampaignStats }) {
  const pct = stats.total > 0 ? (stats.done / stats.total) * 100 : 0
  return (
    <div>
      <progress value={stats.done} max={stats.total} />
      <span>{stats.done}/{stats.total} ({pct.toFixed(1)}%)</span>
      {stats.failed > 0 && <span>{stats.failed} failed</span>}
    </div>
  )
}

// Drill-down: GET /api/task-board/:store_key/work-items?status=failed&limit=50
// Tauri: get_work_items_snapshot(conversation_id, filter)
```

---

## 14. API / Tauri commands

```rust
// GET work items page
pub async fn get_work_items(
    campaign_id: String,
    status: Option<String>,
    offset: u32,
    limit: u32,
) -> Result<WorkItemsPage>;

// GET campaign stats only (lightweight poll)
pub async fn get_campaign_stats(store_key: String) -> Result<CampaignStats>;

// POST continue (optional explicit resume)
pub async fn continue_campaign(conversation_id: String) -> Result<()>;
```

Web route mirror under `agent-pointer/server/`.

---

## 15. Prompt changes (English, runtime)

Add to `task_board/prompts/task_board.md`:

```markdown
## Large enumerations (>50 items, list known)

- Call `task_board_seed_work_items` once with the full item list (or host pre-seeds JSONL).
- Do NOT call `task_board_init` with more than 15 board rows.
- After each atomic unit: `action_verify` (if UI) → `task_board_report_work_item` same turn.
- Board rows represent **batches** only; atomic progress is tracked via report_work_item.
- Final summary: use injected Campaign progress + batch validate_results; unreported items = unverified.

## Dynamic quota (targets chosen at runtime)

- Call `task_board_init_campaign_quota` with `quota_total` and `selection_criteria` (filters, exclusions).
- Do NOT pre-seed N placeholder work_items.
- Per unit: search/filter → `task_board_claim_work_slot` → act → verify → `task_board_report_work_item`.
- If `duplicate_target_key`, pick a different candidate; do not greet the same `target_key` twice.
- Board may be a single `quota_exec` row with `progress done/quota`.
- Final summary: stats.done/quota + validate_results lines; failed items listed from work_items API.

## Sub-agent under campaign (English)

- Use **local** board for UI steps only; do not patch parent milestones.
- Claim/report targets on the **parent campaign** via `task_board_claim_work_slot` / `task_board_report_work_item` (host scopes assignment).
- Read quota and criteria from `[TASK_BOARD_PARENT]`; do not assume the full queue is visible.
- Report breakthroughs in the final assistant content; milestone completion is still host `report_child_status`.
```

---

## 16. Migration and rollout

| Phase | Scope | Flag |
|-------|-------|------|
| A | `WorkItemStore` + SQLite + artifacts | `campaign.workQueueEnabled` default false |
| B | `seed_work_items`, `report_work_item` tools | Computer `allowTools` only |
| B2 | `init_campaign_quota`, `claim_work_slot` (dynamic) | same + recruit-style flows |
| B3 | Gateway `claim_child_work_item` / child-scoped tools | sub-agent + campaign (§3.6) |
| C | Working set snapshot | auto when `work_queue_enabled` |
| D | `CampaignRunner` slice in `session_inner` | settings `campaignAutoSlice` |
| E | UI progress + work item drill-down | always when stats present |

Backward compatibility:

- Boards without `work_queue_enabled` use existing snapshot path unchanged.
- v3 documents validate unchanged; new fields optional with `#[serde(default)]`.

---

## 17. Test plan (pseudocode)

```rust
#[test]
fn seed_1000_items_creates_100_batches_board_shows_first_wave_only() {
    let items: Vec<_> = (1..=1000).map(|i| draft(format!("item {i}"))).collect();
    let out = seed(store_key, items, batch_size: 10).unwrap();
    assert_eq!(out.total, 1000);
    let doc = store.document(store_key);
    assert!(doc.board.len() <= 10);
    assert_eq!(doc.meta.stats.unwrap().total, 1000);
}

#[test]
fn working_set_snapshot_omits_completed_item_details() {
    // complete 500 items, inject snapshot, assert no "item 1".."item 400" in block
}

#[test]
fn resume_after_pause_continues_from_cursor() {
    run_slice(max_items: 20); // completes 20
    assert_eq!(stats.done, 20);
    run_slice(max_items: 20); // completes next 20 without re-doing
}

#[test]
fn batch_complete_advances_wave_and_marks_row_done() {
    // dequeue all items in batch_01, advance_wave, assert batch_02 in_progress
}

#[test]
fn report_work_item_retry_then_fail() {
    // fail 3 times with max_retries=2 → status failed, stats updated
}

#[test]
fn dynamic_quota_claim_dedupes_target_key() {
    init_campaign_quota(quota: 50);
    claim(target_key: "linkedin:A") ok;
    claim(target_key: "linkedin:A") err duplicate;
    report(item, done);
    assert_eq!(stats.done, 1);
}

#[test]
fn dynamic_quota_exhausted_rejects_claim() {
    init_campaign_quota(quota: 2);
    claim + report x2;
    claim #3 → quota_exhausted;
    assert_eq!(doc.meta.status, MetaStatus::Completed);
}

#[test]
fn dynamic_recruit_resume_continues_from_done_count() {
    init_campaign_quota(50);
    run_slice completes 12 greets;
    resume run_slice completes 12 more;
    assert_eq!(stats.done, 24);
}

#[test]
fn child_claim_respects_batch_assignment() {
    seed 20 items batch_01;
    assign task_2 → batch_01;
    child claim on batch_01 ok;
    child claim item in batch_02 err not_assigned;
}

#[test]
fn child_report_updates_parent_stats_not_milestone() {
    child report_child_work_item done;
    assert parent.stats.done == 1;
    assert parent.board.task_2.status == in_progress;
}

#[test]
fn duplicate_target_key_blocked_across_children() {
    parent dynamic quota 50;
    child_a claim target_key X ok;
    child_b claim target_key X err duplicate;
}
```

---

## 18. Open questions (v1.1)

1. **Bulk seed from file**: host tool `campaign_import_jsonl(path)` vs LLM passing 1000 items in one call (size limit → prefer file).
2. **Parallel dequeue**: when `coder` batch has no UI mutex, `dequeue_ready(limit: N)` with `N>1` — Phase 2.
3. **`campaign_append_items`**: extend enumerated campaign after seed — defer.
4. **Dynamic `target_key` extraction**: host helper from URL vs model-only — start model-only; optional host normalize.
5. **Supervisor `AgentTask.work_assignment`**: schema in planner JSON vs host-only dispatch ctx — see §3.6.3.

---

## 19. Summary

| Layer | Storage | Owner |
|-------|---------|-------|
| Campaign | `BoardDocument.meta` | Host + LLM init |
| Mode | `meta.campaign_mode` | `enumerated` (seed) or `dynamic` (quota + claim) |
| Wave | `BoardDocument.board` (≤15 rows) | Host materialize (enumerated) or single `quota_exec` (dynamic) |
| Atomic | `work_items.db` + artifacts | Parent campaign only; child via Gateway (§3.6) |
| Sub-agent | Child board `local_*` | Local patch; claim/report → Gateway |
| Inject | `[TASK_BOARD]` working set | Parent + child `[TASK_BOARD_PARENT]` assignment |
| Loop | `CampaignRunner` | Parent store; child slices via assignment + budget |

This spec preserves task board v3 milestone semantics while adding the **work queue + host slice** pattern used by Deep Agents, durable-agents, and OpenHands durable execution — without requiring an external workflow engine in v1. **Dynamic quota** covers repeat actions with runtime target selection (e.g. recruit greet N people) via `init_campaign_quota` + `claim_work_slot`, not upfront enumeration. **Parent/child** agents share one work_items ledger on the parent campaign; sub-agents execute locally and settle atoms through host Gateway (§3.6).
