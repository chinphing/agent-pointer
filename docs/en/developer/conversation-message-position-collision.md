# Conversation message `position` collisions (context compression)

English | [简体中文](../../zh-CN/developer/conversation-message-position-collision.md)

> **Status**: Fixed (2026-07, `persist_context_compression` + orphan-aware `sync`).  
> **Related**: [`../design/conversation-store-append-migration.md`](../../zh-CN/design/conversation-store-append-migration.md)

## 1. Symptom

After several rounds of context compression in a long conversation, the SQLite `messages` table shows many rows where **several different `message_id`s hang off the same `position`**:

- `message_id` itself is not duplicated (`UNIQUE(conversation_id, message_id)` still holds)
- In a colliding pair, the "new" side is usually `ctx_*` (the compression summary user row), and the "old" side is usually an earlier `tool` / `assistant`
- By timestamp, `ctx_*` is always later than the old row it displaces
- `ORDER BY position ASC` gives a nondeterministic reload order, and a search window taken by `position` can be scrambled too

Typical ratio (one real database investigation): out of about 791 message rows, several hundred `position`s collided.

## 2. Root cause (not "leftovers from the old sync")

`sync_messages_ordered_with_meta` does this to the list passed in:

```text
INSERT … ON CONFLICT(conversation_id, message_id) DO UPDATE SET position = …
```

That is, it **upserts by `message_id`** and writes the list index as `0..n-1`.  
It **does not DELETE** rows that are not in this list.

The compression path used to run in this order:

1. Mark the prefix soft-exclude (`context_state.included = false`)
2. Insert the `ctx_*` summary at the split point
3. **`history.drain(..split)`** drops the prefix (to save memory)
4. **`sync_ordered(shortened history)`** ← the problem

The short list contains only `[ctx_, …suffix]`, so it is rewritten to low `position` numbers; the prefix's old rows stay in the DB on their original numbers → **two rows at the same `position`**.  
Every compression repeats this, so collisions accumulate.

```text
DB before compression:  pos0=A  pos1=tool  pos2=B
sync after drain:       [ctx, B] → writes pos0=ctx, pos1=B
Result:                 pos0 = A coexists with ctx; pos1 = tool coexists with B
```

Notes:

- The design intent of soft-exclude is that **the row stays in the DB** (visible in the UI, total row count unchanged); it is simply not fed to the model
- What is wrong is **using the already-drained subset to remap indexes across the whole table**, not "whether exclude rows should be kept"

## 3. Approaches that look plausible during investigation

| Approach | Conclusion |
|------|------|
| `DELETE … message_id NOT IN (short list)` after sync | Physically deletes soft-exclude history; the lead's short list also wrongly hits scoped sub-agent rows in the same conversation |
| Never change `position` on existing rows; new rows always get `max+1` | Removes the collision, but `ctx_` lands at the end of the queue and the split order is wrong |
| Let `ctx` and the split message share a `position`, with the timestamp as second sort key | Sharing with "the row before B" plus `ASC` can theoretically order them, but it requires changing `ORDER BY` across the whole chain, and existing collisions stay scrambled |
| `DELETE` the whole conversation and INSERT from memory | Conflicts with append-only / soft-exclude / the shared sub-agent row table |

## 4. Correct fix (current implementation)

### 4.1 Compression-specific persistence: `persist_context_compression`

**Before drain** (memory still contains the prefix plus the already-inserted `ctx_`):

1. **UPDATE** the payloads of the prefix rows already marked exclude (**without** changing their `position`)
2. For all rows at and after the split point: `position += 1` (shift the whole block back)
3. **INSERT** `ctx_*`, taking the split point's original number (strictly less than the suffix's first row B)

Then `history.drain(..split)` releases the memory.  
**Never** run a `0..n-1` `sync_ordered` on the post-drain short list again.

Split point = `insert_before_message_id` (`history[split]`, i.e. the first row of the kept segment).

New messages (including parallel sub-agent appends) still follow the existing rule: `position = MAX(position) + 1` (computed inside a single transaction).

### 4.2 Guards in the later `sync_ordered`

If the DB still has rows **outside history** (typically drained soft-exclude rows):

- **Do not** remap the short list to `0..n-1`
- Existing id: update the payload only, **keep** the original `position`
- New id: insert before the following neighbour's position (shifting it back first), otherwise `max+1`

This way the tool-pass flush cannot collide with exclude rows through a short list again.

### 4.3 Code entry points

| Module | Responsibility |
|------|------|
| `context_compression/` (`run.rs`) | Calls `persist_compression_splice` before drain |
| `conversation_transcript/mod.rs` | `persist_compression_splice` wrapper |
| `conversation_store/write.rs` | `persist_context_compression_in_conn`, orphan-aware `sync_messages_ordered_with_meta_in_conn` |

## 5. Interaction with parallel sub-agents

- Sub-agents and compression persistence both go through `execute_write` / `BEGIN IMMEDIATE`, so **writes to the same DB are serialized** and two transactions cannot read the same `max` and each insert a row.
- Main-conversation compression and a `run_subagent` parallel wave are usually offset in time (during a wave the lead is waiting for results).
- A sub-agent's own compression (`CompressionScope::SubAgent`) **does not** go through `persist_context_compression`; scoped rows share the table with the lead, so when the main compression shifts the block, any row with `position >= split point` gets +1 too, preserving relative order.

## 6. Regression notes

- After compression: `COUNT(*)` should be "original row count + 1 (summary)"; `GROUP BY position HAVING COUNT(*) > 1` should be empty; `ctx_*` should sit between the exclude prefix and the kept suffix.
- Unit tests in `conversation_store::write::tests`: `persist_compression_shifts_suffix_and_keeps_unique_positions`, `short_list_sync_with_db_orphans_does_not_collide_positions`.
- **Existing databases** with already-collided rows do not clean themselves up; a separate data repair is needed when required (organise by `created_at` / whether the row is `ctx_`, etc.). This fix only stops new collisions.

## 7. Contract (for future changes)

1. Soft-exclude rows must be writable back to the DB (at least updating `context_state`), and should keep an independent `position`.  
2. When inserting a summary at the split point, ensure `ctx.position < suffix first row .position` (currently done by shifting the whole suffix by +1).  
3. The in-memory drain exists only to save RAM; it **must not** become a reason for "the DB's list of truth gets shorter".  
4. Do not do a dense remap on a list that is "obviously shorter than the DB row set"; if a full re-layout is truly required, the list passed in must contain every id that should be kept (including exclude and scoped rows), or switch to explicit point DELETEs.
