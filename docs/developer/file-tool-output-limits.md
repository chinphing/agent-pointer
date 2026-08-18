# File 工具输出上限（`file_read` / `file_grep`）

默认值（可在 **设置 → 系统设置** 修改）：

| 项 | 默认 | 设置字段 | 允许范围 |
|----|------|----------|----------|
| 正文 | 64 KiB | `fileReadMaxBytes` | 4 KiB – 1 MiB |
| 单行 | 1 KiB | `fileLineMaxBytes` | 256 B – 16 KiB |
| 搜索命中 | 50 | `fileGrepMaxResults` | 1 – 200 |

Agent 不能靠把 `maxBytes` / `maxResults` 调大来突破**当前**天花板。
工具参数只能下调。

实现：`crates/pointer-core/src/tools/file/{mod,read,grep}.rs`；
设置落在 `UserSettings`，桌面与 Web 共用。

## 行为

- **`file_read`**：无行窗口且整文件大于正文上限 → **报错拒绝**；有行窗口则只返回窗口并截断。
- **`file_grep`**：命中条数、单行 snippet、命中合计体积（与正文上限相同）均截断。
- 单文件 **> 2 MiB** 仍跳过（实现常量，不计设置），计入 `skippedLargeFileCount`。

## 观测

截断与跳过大文件打 **info / warn** 日志，响应带 `truncated` / `warning`。
