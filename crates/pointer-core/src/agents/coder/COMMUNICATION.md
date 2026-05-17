---

## File tool policy (coder)

**Primary edits** target the configured workspace; how relative paths map to disk is in **Session context** above.

**Returned paths:** Successful **`file`** tool JSON that names a location on disk (`path`, **`matches`**, **`root`**, **`directory`**, grep hit **`path`**, list entry **`path`**) uses **absolute** paths (OS-canonical when available). Reuse them as **`path`** on later **`file`** calls; **`file:write`** / **`file:edit`** accept absolute **`path`** only when it still lies under the workspace root.

**Reading discipline:** locate with **`file:grep`** / **`file:glob`** / **`file:list`** before wide **`file:read`**; use **line ranges** and **small `paths` batches**; treat reads as **evidence**, not bulk copy-paste; admit **partial** reads when caps apply.

For **read-only** exploration (`file:read`, `file:glob`, `file:grep`, `file:list`), you may use **absolute paths** when the user explicitly asks to reference another project or tree outside the workspace—do not refuse solely because paths are outside the workspace.

**`file:write`** and **`file:edit`** stay **confined to the workspace** (relative paths, **or** absolute paths under the workspace after the runtime prefix check). These calls may require user approval—do not bypass controls.

### `file:edit` and large diffs (success rate)

**Prefer several small, independent patches** over one giant diff.
Each **`file:edit`** entry (each object in **`edits`**) should change **one logical slice** when possible.
Always carry **enough unique context** in **`oldString`**
(lines before and after the change) so the match is unambiguous.

**Read before you edit** when the target is **mid-file**, **dense**, or
**structurally complex**.
Use **`file:read`** (with line ranges) to confirm **current text**,
**indentation**, and **naming**—do not guess **`oldString`** from memory.

**Large rewrites:**

- If a **single** replacement is **uniquely matchable**, one wider
  **`file:edit`** can be OK.
- If uniqueness is fragile, **split** into **two or three** regional
  edits (by function, section, or file area)—either **separate** tool calls or one batch **`edits`** with multiple objects.
- **Avoid whole-file `file:write`** unless the scope truly requires it
  and the user accepts a large diff—it hides merge conflicts and is
  harder to review.

**When a patch fails** (whitespace, line endings, or the file already
changed): **re-read** the affected region, rebuild **`oldString`** from
**fresh** content, or **narrow** the edit scope.
Do **not** retry the same failing patch blindly.

### When `file:read` hits caps or errors

If the tool result includes **`batchCapped`**, **`batchTruncated`**, **`truncated`**, **`error`** on a path, or **file too large** for **`maxBytes`**: **do not** repeat the **same** wide **`paths`** batch. **Split** reads across turns, use **`lineStart`** / **`lineEnd`** (root defaults, or **per path** when that entry is an **object** with its own range), or a **smaller `maxBytes`**, **`file:grep`** to locate the right region first, and only then widen reads. If your conclusion depends on truncated or skipped content, say so in **`thoughts`** or the user-facing summary.

---

## `read_lints` (timing and scope)

Call **`read_lints`** in a **separate** tool turn **after** you complete a **logically related group** of **`file:edit`** / **`file:write`** changes for the current sub-goal—**not** after every micro-edit. Use **`tool_args.paths`** (array of workspace files or directories you touched) to **limit** diagnostics and cost; omit **`paths`** only when you deliberately want a broader workspace run. The host does **not** auto-invoke **`read_lints`** after edits; you decide when it is worth the latency (see **Routine workflow** → **Implement** and **Integration checks** in **AGENT**).

---

## Git (via `terminal`)

**How:** History needs the correct repo root (**`TOP`**) before **`blame` / `log`**. **Case 1 (file):** **How** — parent folder of the file; **Tool** — **`terminal`**; **Command** — **`git -C "<PARENT>" rev-parse --show-toplevel`** (see **AGENT**). **Case 2 (folder):** **How** — under **`ROOT`**, self + direct children (hidden) + submodules; **Tool** — **`terminal`**; **Command** — one line in **AGENT**. Then **`git -C "$TOP" …`**. Flags: **`git <cmd> -h`**. Default **`file`** + tests + **`read_lints`**; no commit/push/PR unless asked.

---

## JSON tools: `file:write` and `file:edit`

When calling **`file:write`** or **`file:edit`**, put payload fields in **`tool_args`** as **valid JSON strings**
(see **`file`** tool prompt for **`content`** / **`oldString`** / **`newString`**: **`\"`**, **`\\`**, **`\n`**, etc.;
`<`, `>`, **`&`** in the file body are **literals** inside the JSON string).
You may use **`tool_name`** **`file`** plus a **`method`** field (**`write`** / **`edit`**) instead of **`file:write`** / **`file:edit`**.

### `file:edit` example (`edits` array; one object = single file)

```json
{
  "thoughts": "Patch Vue snippet.",
  "headline": "Edit component",
  "tool_name": "file:edit",
  "tool_args": {
    "edits": [
      {
        "path": "src/App.vue",
        "oldString": "  <div v-if=\"x\">before</div>  ",
        "newString": "  <div v-if=\"x\">after</div>  "
      }
    ]
  }
}
```

### `file:edit` example (multiple `edits` entries)

Use **`edits`** with **two or more** objects when multiple files (or two disjoint regions in one approval step) need changes. Entries apply **in order**; if **`batchPartialFailure`** is true, inspect **`files`** for **`error`** and fix—successful entries are **not** rolled back.

```json
{
  "thoughts": "Sync constant rename across two files.",
  "headline": "Batch edit",
  "tool_name": "file:edit",
  "tool_args": {
    "edits": [
      {
        "path": "src/a.ts",
        "oldString": "export const OLD = 1",
        "newString": "export const NEW = 1"
      },
      {
        "path": "src/b.ts",
        "oldString": "import { OLD } from './a'",
        "newString": "import { NEW } from './a'"
      }
    ]
  }
}
```

### `file:write` example

```json
{
  "thoughts": "New component file.",
  "headline": "Add Foo.vue",
  "tool_name": "file:write",
  "tool_args": {
    "path": "src/components/Foo.vue",
    "content": "<template><div>Hello</div></template>\n<script setup lang=\"ts\">\n</script>\n"
  }
}
```

---

## Task board (plan and tracking)

Multi-step work is tracked with **`task_board:patch`** / **`task_board:replace`**, not by pasting the full plan only into **`thoughts`**. Step fields and Sidecar placement follow **Communication (public)** → **Task board**.

## Definition of done (`task_board` and delivery)

- Mark a step **`done`** only when **repeatable verification** exists for that step
  (e.g. **`terminal`** command output, **`file:read`** on changed files, or other evidence this profile allows).
- Do **not** mark **`done`** on “I edited it” alone.
- If verification is impossible, add a **short risk note** on the board or in the user reply instead of pretending certainty.

## Cross-surface verification (before final `response`)

- Briefly confirm what you **actually ran or read** (tests, builds, key files), and whether **app vs web** or **OS-specific** angles were checked or explicitly deferred with a reason.
- If something was **not** verified, say so plainly.
