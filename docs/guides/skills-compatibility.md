# Skills 生态兼容（Codex / Claude / Cursor）

Pointer 采用社区通用的 **`SKILL.md`** 格式（YAML frontmatter + Markdown 正文），与 OpenAI Codex、Claude Code、Cursor 等工具的 skills 目录结构兼容。

## 自动发现路径

`reload_meta` / 启动加载时按下列目录扫描子文件夹（含 `SKILL.md` 或 `skill.md`）。**同名 skill 以先扫描到的为准**（应用内导入目录优先级最高）。

| 优先级 | 路径 | 来源 |
|--------|------|------|
| 1 | `{data_dir}/skills/` | Pointer 应用内导入 |
| 2 | `./.cursor/skills/` | Cursor 项目 skills |
| 3 | `./.claude/skills/` | Claude Code 项目 skills |
| 4 | `./.agents/skills/` | Codex / Agent 标准项目 skills |
| 5 | `./skills/` | 通用项目 skills |
| 6 | `~/.cursor/skills/` | Cursor 用户 skills |
| 7 | `~/.claude/skills/` | Claude Code 用户 skills |
| 8 | `~/.agents/skills/` | Codex 用户 skills |
| 9 | `$CODEX_HOME/skills/` 或 `~/.codex/skills/` | Codex CLI 用户 skills |

**不扫描**：以 `.` 开头的目录（如 Codex `.system`）、Cursor 内置目录 `skills-cursor`。

## Frontmatter 兼容

| 字段 | 说明 |
|------|------|
| `name` | 必填，skill id |
| `description` | 必填，支持 `>` / `|` 多行 YAML |
| `allowed-tools` / `allowed_tools` | 可选；记录为 `toolNames` 元数据（不强制注册为 Pointer 工具） |
| `tags` / `metadata.tags` | 可选；合并为 UI 标签 |
| `license` / `compatibility` | 可选；校验长度，不参与运行时 |
| 其它字段（`version`、`triggers`、`disable-model-invocation` 等） | 忽略，不报错 |

## 资源目录

与 Codex / Claude 一致，支持同目录下的 `references/`、`scripts/`、`assets/` 等文件；通过 **`skill_read_resource`** 按需读取（不自动执行脚本）。

## 手动导入

技能库 UI 或 **`skill_import`** 仍可将 zip / 本地目录安装到 `{data_dir}/skills/`，安装后覆盖同 id 的外部发现结果。

## 运行时范围

Skills 仅在 **general** lead agent 下加载；详见 [skills-persistence.md](skills-persistence.md)。

## 跨平台

macOS / Windows / Linux 使用相同路径约定（`~` 为用户主目录，`CODEX_HOME` 可覆盖 Codex 根目录）。
