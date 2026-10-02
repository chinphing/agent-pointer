# Reviewing this turn's file changes (turn baseline)

English | [简体中文](../../zh-CN/developer/turn-file-baseline-review.md)

## Behavior

1. **Tool result and UI are decoupled**: the tool result of `file_edit` / `file_write` **does not contain** `diff_lines` (`path` / `success` / `stats`; edit also has `replaced`, write also has `bytesWritten` / `created`). The Diff fragment on the tool row is computed on the fly by the frontend from the `oldString`/`newString` in the arguments; the whole-file net diff is computed lazily from the turn baseline.
2. **Turn baseline**: within a **main-Agent user turn** of the same conversation, before the **first** `file_edit` / `file_write` on a path, the full text at that moment (empty if newly created) is written into the app data directory.
   - `turn_id` must be the lead's user message id; a sub-Agent's host stub / scoped user row **must not** be used as the anchor (otherwise you get "no snapshot found for before this turn's changes").
   - A `file_edit` / `file_write` inside a sub-Agent also goes through the registry's `TurnBaselineGuard` and shares the above `turn_id` with the main Agent.
3. **Conversation footer**: at the end of each turn it lists the files changed in that turn (including scoped sub-Agents).
   Clicking opens the workspace panel on the right.
   It compares "this turn's baseline ↔ the next baseline for the same path after it"; if there is no next one, it compares against the disk.
   For the interaction and copy see [`../ui/turn-change-summary.md`](../../zh-CN/ui/turn-change-summary.md).
4. **Right sidebar main navigation**: the folder / Git icons (hover: workspace files, changed files). This turn's net diff goes through the preview Tab.
5. **Git "changed files" preview**: a whole-file structured diff (unchanged lines collapsible), not `git diff` hunk text.
   - Unstaged / untracked present → index (or empty) ↔ workspace disk
   - Staged only → HEAD ↔ index
   - `MM` prefers the unstaged side; a single side is capped at about 2MB
   - API: `get_workspace_git_diff` / `GET /api/workspace/git/diff` (optional `status`) → `diffLines` / `diffStats` / `mode`

## Storage

`{app_data}/turn-baselines/{conversation_id}/{turn_id}/{sha256(path)}.txt`

- `turn_id` = that turn's lead user message id (the same anchor as the task board main turn; `latest_real_user_message_id` skips scoped / synthetic user rows)
- Per-file cap 5MB; over the cap it is skipped with a warn
- Capture relies on the `TurnBaselineGuard` the registry sets when `file_edit` / `file_write` is called
- Side files:
  - `.path`: absolute path (for debugging / footer fallback when there is no `summary.json`, no +/-)
  - `summary.json`: the footer summary (written once when the UI freezes that turn; the same content as the on-screen list)

## API

- Tauri: `get_turn_file_diff` (`spawn_blocking`, to avoid blocking the UI async runtime)
- HTTP: `GET /api/workspace/turn-file-diff?conversationId=&turnId=&workspaceRoot=&path=`

Returns `diffLines` / `diffStats` / `baselineMissing` / `created` (camelCase).

- Tauri: `save_turn_file_changes` / `list_turn_file_changes`
- HTTP:
  - `POST /api/workspace/turn-file-changes` body `{ conversationId, turnId, files }`
  - `GET /api/workspace/turn-file-changes?conversationId=&turnIds=id1,id2`

`files` is `[{ path, kind, adds, dels }]` (camelCase).
It only reads and writes the baseline directory; it does **not** `load_messages` and does **not** hydrate scoped rows.

### When finding the "next turn baseline", do not read all messages

Comparing "this turn's baseline ↔ the next baseline for the same path after it" needs the list of subsequent **lead user turn ids**.
The implementation must use a lightweight query (`message_id` + `content` + `is_scoped` / `position`)
and **must not** `load_messages` the whole conversation and then deserialize every `payload`.

In a long conversation, a single conversation in `conversations.db` can reach tens of thousands of rows and hundreds of MB of payload;
loading everything makes clicking "changed files" to open the right-hand diff noticeably slower (hard to notice in short conversations).

### The turn id when writing a file should not be queried in full every time

`turn_id` is exactly this turn's lead user message id and does not change within a turn.
At the start of `run_chat`, read it once from the working set already in memory and remember it (keyed by `conversation_id`).
Afterwards the `file_edit` / `file_write` of the same run (including sub-Agents) reuse it directly, then set `TurnBaselineGuard`.

After a process restart, or when writing before it has been remembered: read `message_id` + `content` from the tail by `position`
(`role=user` and `is_scoped=0`), skip synthetic user rows, and remember it once matched.
Do not use `is_system_generated`, and do not `load_messages`.
When there is no real user message, use `orphan-{conversation_id}`, and do not remember that placeholder id.

## Performance

| Path | Expected cost | Forbidden |
|------|----------|------|
| `ensure_baseline` | one full-text write to disk per path per turn (already the case) | — |
| `save_turn_file_changes` | writes `summary.json` once per successful turn freeze | writing a summary on every file_edit; a failure must not block the UI |
| `list_turn_file_changes` | reads `summary.json` (or the `.path` fallback); batched by turnIds | pulling messages / scoped rows for the footer |
| `turn_file_diff` | reads two full texts + diff; triggered only when Review is clicked | computing whole-file diffs in bulk for the footer list |
| the `turn_id` at write time | reuse this turn's remembered lead user message id; one lightweight tail query when not remembered | `load_messages`; using `is_system_generated` as the anchor |

For the footer display strategy see the "Performance" section of [`../ui/turn-change-summary.md`](../../zh-CN/ui/turn-change-summary.md).

## Cross-platform

Desktop and web share the same core logic and API; both must be able to reach the conversation workspace's disk path.

## Known issues (not addressed for now)

When the same workspace and different conversations change the same file, the **footer file list does not mix them up** (it is keyed by conversation message / `{conversation_id}/{turn_id}` baseline).
When "there is no baseline for the same path later in this conversation", Review reads the **disk** on the right, and that does mix them up.

Example:

1. A changes file X (this conversation never changes X again afterwards → the right side = the disk)
2. B changes the same file X
3. A changes X again

Before and after step 2, opening the diff of A's **first** turn again shows B's written content on the right, different from what it was right after step 1.
After step 3, the right side of A's first turn changes to the baseline from before A's second change (the disk was B's version at that time) and no longer follows the live disk; A's second turn is the one that shows "B's version → after A changed it again".

Open question: whether historical turns should freeze a "finished" snapshot, so they are not dragged along by another conversation's disk writes.
