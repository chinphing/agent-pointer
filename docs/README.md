# 文档索引

`docs/` 下按主题分子目录，便于与实现对照和维护。

| 目录 | 内容 |
|------|------|
| [**design/**](design/README.md) | 设计方案、路线图、实现计划、技术提案（评审 / 估人天 / 对照实现） |
| [**internals/**](internals/README.md) | 运行时内部机制：提示词拼接顺序、扩展钩子、任务板与 verification 约定 |
| [**agents/**](agents/README.md) | 各 Agent 专题（Computer 提示词、Coder Git 策略等） |
| [**llm/**](llm/README.md) | LLM 调用观测、调试落盘、thinking API 等 |
| [**guides/**](guides/README.md) | 使用说明与配置：`run_subagent` / `allowAgents`、混合 `read_lints`、离线评测搭建 |
| [**ui/**](ui/README.md) | 前端界面与交互约定 |

从仓库根目录引用示例：`docs/guides/pointer-lint-config.md`。
