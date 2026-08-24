# 工具内容上限（读文件 / 搜索 / 终端）

默认值（可在 **设置 → 系统设置** 的内容上限 / 终端超时修改）：

| 项 | 默认 | 设置字段 | 允许范围 |
|----|------|----------|----------|
| 读文件正文 | 64 KiB | `fileReadMaxBytes` | 4 KiB – 1 MiB |
| 单行 | 1 KiB | `fileLineMaxBytes` | 256 B – 16 KiB |
| 搜索命中 | 50 | `fileGrepMaxResults` | 1 – 200 |
| 终端 stdout/stderr（各一路） | 16 KiB | `terminalOutputMaxBytes` | 4 KiB – 256 KiB |
| 终端空闲超时 | 30 秒 | `terminalTimeoutSeconds` | 1 – 86400 秒 |
| 终端墙钟上限 | 24 小时 | `terminalMaxWallHours` | 1 – 10000 小时 |

Agent 不能靠把 `maxBytes` / `maxResults` / `maxOutputBytes` / `maxWallMs` 调大来突破**当前**天花板。
工具参数只能下调。

实现：`crates/pointer-core/src/tools/file/{mod,read,grep}.rs`、
`crates/pointer-core/src/tools/terminal.rs`；
设置落在 `UserSettings`，桌面与 Web 共用。

## 行为

- **`file_read`**：无行窗口且整文件大于正文上限 → **报错拒绝**；有行窗口则只返回窗口并截断。
- **`file_grep`**：命中条数、单行 snippet、命中合计体积（与正文上限相同）均截断。
- 单文件 **> 2 MiB** 仍跳过（实现常量，不计设置），计入 `skippedLargeFileCount`。
- **`terminal`**：stdout / stderr **各自**截到上限；超限**留尾巴、丢开头**，前缀
  `...[output truncated]`。实时预览不按此上限截；回给模型的工具结果按此截。
  空闲超时与墙钟上限由设置控制；工具 `timeoutMs` / `maxWallMs` 只能下调。

## 观测

截断与跳过大文件打 **info / warn** 日志，响应带 `truncated` / `warning`
（终端为 `stdoutTruncated` / `stderrTruncated`）。
