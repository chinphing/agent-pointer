# File 工具写入范围（`file_write` / `file_edit`）

实现：`crates/pointer-core/src/tools/file/path.rs` → `resolve_writable_path`。

## 相对路径

仍解析为**当前工作区**（传入的 `workspace_root` / 会话工作区）下的路径；禁止用 `..` 逃出工作区。

实现注意：
- 允许写入根列表（`writable_path_roots`）会合并更宽的目录（如系统 `temp_dir`、home）。
  会话工作区必须始终保留为列表首项，且合并更宽根时不得被剔除。
- 相对路径解析必须以传入的 `workspace_root`（canonical）为准，不得改用列表的
  `first()` 以外的隐式来源；即便如此也应保证 `roots[0]` 即会话工作区。

## 绝对路径 / `~`

可写入以下根目录及其子路径（canonical 前缀校验）：

| 根 | 说明 |
|----|------|
| 工作区 | 会话 `workspaceRoot` 或进程 cwd |
| 用户主目录 | `dirs::home_dir()`，`~` 会展开到此 |
| 系统临时目录 | `std::env::temp_dir()`（含 `TMPDIR` 等） |
| 用户数据 / 配置 / 缓存 | `dirs::data_dir()`、`config_dir()`、`cache_dir()` |
| 桌面 / 文档 / 下载 | `dirs::desktop_dir()`、`document_dir()`、`download_dir()` |
| Pointer 应用数据 | `{data_dir}/PointerApp`（或 Dev 构建下的 `PointerAppDev`） |

主目录已覆盖桌面/文档/下载时，子目录不会重复加入列表。

## 只读工具

`file_read` / `file_glob` / `file_grep` / `file_list` 仍可通过绝对路径读取工作区外的**已存在**路径（见 `resolve_accessible_path`）。

`file_read` / `file_grep` 的返回体有**硬上限**（调用方无法突破），见 [file-tool-output-limits.md](file-tool-output-limits.md)。

## 未包含（有意限制）

- 系统目录（如 `/etc`、`C:\Windows`）
- 其他用户的主目录
- 未挂载 / 不可 canonical 的路径

**Agent 策略（唯一来源）：** general lead 何时本地用 `file_*`、何时
`run_subagent(coder)`，只维护在
`crates/pointer-core/src/tools/prompts/run_subagent.md` 的 **`coder`** 小节。
其他 agent / 公共提示 / 本文只引用，不复述细则。
`general` 的 `allowTools` 含常用 `file_*`（含 `file_write` / `file_edit`）；
加载技能用 **`skill_read` / `skill_import`**（留在 general）。
