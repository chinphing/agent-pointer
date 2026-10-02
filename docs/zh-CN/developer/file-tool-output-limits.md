# 工具内容上限（读文件 / 搜索 / 终端）

[English](../../en/developer/file-tool-output-limits.md) | 简体中文

默认值（除读文件行数、glob / 列举条数外，可在 **设置 → 系统设置** 的内容上限 / 终端超时修改）：

| 项 | 默认 | 设置字段 | 允许范围 |
|----|------|----------|----------|
| 读文件正文 | 64 KiB | `fileReadMaxBytes` | 4 KiB – 1 MiB |
| 读文件行数 | 默认 500，上限 2000 | （实现常量，非设置项） | 1 – 2000 |
| 单行 | 1 KiB | `fileLineMaxBytes` | 256 B – 16 KiB |
| 搜索命中 | 50 | `fileGrepMaxResults` | 1 – 200 |
| glob 命中 | 默认 100，上限 500 | （实现常量） | 1 – 500 |
| 目录列举 | 默认 100，上限 2000 | （实现常量） | 1 – 2000 |
| 终端 stdout/stderr（各一路） | 16 KiB | `terminalOutputMaxBytes` | 4 KiB – 256 KiB |
| 终端空闲超时（未传 `timeoutMs` 时的默认） | 30 秒 | `terminalTimeoutSeconds` | 1 – 86400 秒 |
| 终端墙钟上限 | 24 小时 | `terminalMaxWallHours` | 1 – 10000 小时 |

Agent 不能靠把 `maxBytes` / `limit` / `maxOutputBytes` / `maxWallMs` 调大来突破**当前**天花板。
工具参数只能下调。空闲超时例外：`timeoutMs` 可高于设置默认值，硬上限 86400 秒。

实现：`crates/pointer-core/src/tools/file/{mod,read,grep,glob,list}.rs`、
`crates/pointer-core/src/tools/terminal.rs`；
设置落在 `UserSettings`，桌面与 Web 共用。

## 行为

- **`file_read`**：每次都有行窗口。未传时 **`offset` 默认 1**、**`limit` 默认 500**；`limit` 上限 **2000**（工具参数不能再抬）。
  只返回该窗口，正文仍受 **`maxBytes`** 截断。窗口读完即停，不再扫完整文件。
  对外名称只有 **`offset` / `limit`**（`offset` 为 1-based 起始行，`limit` 为最多返回行数，JSON **integer**）。
  Schema `additionalProperties: false`，运行时也不再认 `lineStart` / `lineEnd` / `startLine` / `endLine`。
- **`file_grep`**：命中条数工具参数为 **`limit`**（默认与设置 **`fileGrepMaxResults`** 相同，**50**；上限 **200**，工具参数不能再抬）。
  单行 snippet、命中合计体积（与正文上限相同）均截断。
- **`file_glob`**：命中条数工具参数为 **`limit`**（默认 **100**，上限 **500**）。
- **`file_list`**：返回条数工具参数为 **`limit`**（默认 **100**，上限 **2000**）。
- 单文件 **> 2 MiB** 仍跳过（实现常量，不计设置），计入 `skippedLargeFileCount`。
- **`terminal`**：stdout / stderr **各自**截到上限；超限**留尾巴、丢开头**，前缀
  `...[output truncated]`。回给模型的工具结果按此截。
  前端实时缓冲按同一上限留尾部，前缀相同。命令结束后这份缓冲和参数、结果一起按正文规则决定去留，见
  [`../ui/tool-payload-memory.md`](../ui/tool-payload-memory.md)。
  空闲超时：未传 `timeoutMs` 时用设置默认值；工具参数可在 1s–86400s 内指定，不受设置默认值封顶。
  墙钟上限由设置控制；工具 `maxWallMs` 只能下调。

## 观测

截断与跳过大文件打 **info / warn** 日志，响应带 `truncated` / `warning`
（终端为 `stdoutTruncated` / `stderrTruncated`）。
