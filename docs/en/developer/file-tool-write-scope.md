# File tool write scope (`file_write` / `file_edit`)

English | [简体中文](../../zh-CN/developer/file-tool-write-scope.md)

Implementation: `crates/pointer-core/src/tools/file/path.rs` → `resolve_writable_path`.

## Relative paths

They still resolve to a path under the **current workspace** (the `workspace_root` passed in / the session workspace); using `..` to escape the workspace is forbidden.

Implementation notes:
- The allowed write-root list (`writable_path_roots`) merges wider directories (such as the system `temp_dir`, home).
  The session workspace must always stay as the first entry in the list and must not be dropped when wider roots are merged.
- Relative path resolution must be based on the `workspace_root` passed in (canonical) and must not switch to an implicit source other than the list's
  `first()`; even so, `roots[0]` should be guaranteed to be the session workspace.

## Absolute paths / `~`

The following roots and their sub-paths are writable (canonical prefix validation):

| Root | Description |
|----|------|
| Workspace | the session `workspaceRoot` (the process cwd is not used) |
| User home directory | `dirs::home_dir()`; `~` expands to this |
| System temp directory | `std::env::temp_dir()` (including `TMPDIR` etc.) |
| User data / config / cache | `dirs::data_dir()`, `config_dir()`, `cache_dir()` |
| Desktop / Documents / Downloads | `dirs::desktop_dir()`, `document_dir()`, `download_dir()` |
| Pointer app data | `{data_dir}/PointerApp` (or `PointerAppDev` under a Dev build) |

When the home directory already covers Desktop/Documents/Downloads, those sub-directories are not added to the list again.

## Read-only tools

`file_read` / `file_glob` / `file_grep` / `file_list` can still read an **existing** path outside the workspace via an absolute path (see `resolve_accessible_path`).

The response bodies of `file_read` / `file_grep` / `terminal` have a **hard ceiling** (callers cannot break through it); see [file-tool-output-limits.md](file-tool-output-limits.md).

## Not included (intentional limits)

- System directories (such as `/etc`, `C:\Windows`)
- Other users' home directories
- Paths that are not mounted / cannot be canonicalised

**Agent policy (single source of truth):** when the general lead uses `file_*` locally versus
`run_subagent(coder)` is maintained only in the **`coder`** subsection of
`crates/pointer-core/src/tools/prompts/run_subagent.md`.
Other agents / shared prompts / this page only reference it and do not restate the details.
`general`'s `allowTools` includes the common `file_*` tools (including `file_write` / `file_edit`);
loading skills uses **`skill_read` / `skill_import`** (which stay on general).
