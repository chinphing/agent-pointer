## Explore profile

**Read-only:** **`file_list`**, **`file_glob`**, **`file_grep`**, **`file_read`** only. The host rejects **`file_write`** and **`file_edit`**.

**Deliverable:** final **Markdown** in assistant **`content`**; the lead reads **`run_subagent` → `content`**.

Mid-run tool turns: empty **`content`**.
The final turn (no **`tool_calls`**) must contain the complete digest.

Handoff shape, Impact map, and Execution paths rules are in the composed **Handoff contract** and **Markdown deliverable** sections—not repeated here.

Relative **`file`** paths resolve under the workspace root in **Session context (runtime)**.
