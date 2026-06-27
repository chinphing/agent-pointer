# Skills 生态兼容（Codex / Agent 标准）

Pointer 采用社区通用的 **`SKILL.md`** 格式（YAML frontmatter + Markdown 正文），与 OpenAI Codex、`.agents/skills` 等 Agent 标准目录结构兼容。不自动扫描 Cursor（`.cursor/skills`）或 Claude Code（`.claude/skills`），避免与本机其它 IDE 的 skills 互相干扰。

## 自动发现路径

`reload_meta` / 启动加载时按下列目录扫描子文件夹（含 `SKILL.md` 或 `skill.md`）。**同名 skill 以先扫描到的为准**（优先级从高到低）：

| 优先级 | 路径 | 来源 | `provenance` | 可 `skill_patch` |
|--------|------|------|--------------|------------------|
| 1 | `~/.pointer/skills/` | 用户库（创建 / 导入 / Agent 写入） | `user` | ✅（非 pinned） |
| 2 | `~/.agents/skills/` | 用户 Codex / Agent 标准目录 | `external` | ❌ |
| 3 | `{data_dir}/PointerApp/skills/` | 应用 bundled 同步副本 | `system` | ❌ |

**不扫描**：工作区 `{workspace}/.agents/skills/`、工作区 `./skills/`（Pointer 内置 skill 源码）、以 `.` 开头的 vendor 目录。

**修改 external skill**：用 **`skill_import`** 复制到 `~/.pointer/skills/`，或 **`skill_patch`** 仅作用于用户库中已存在的同名 skill。

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

与 Codex / OpenClaw 一致，支持同目录下的 `references/`、`scripts/`、`assets/` 等文件；通过 **`skill_read`**（带 `path`）按需读取（不自动执行脚本）。

## 运行时注入（OpenClaw 对齐）

启用技能时，system 注入 **`<available_skills>`**  catalog，每个 skill 含：

| 字段 | 含义 |
| --- | --- |
| `<name>` | skill id；**`skill_read`** 的 **`skill_id`** |
| `<description>` | frontmatter 摘要 |
| `<location>` | `SKILL.md` 路径（home / app data 展示为 `~/…`） |

技能根目录 = **`dirname(<location>)`**。正文中的 `{baseDir}` 在 **`skill_read`** 加载时替换为绝对路径；`terminal` 跑脚本时使用绝对路径。

## 手动导入

技能库 UI 或 **`skill_import`** 仍可将 zip / 本地目录安装到 **`~/.pointer/skills/`**，安装后覆盖同 id 的外部 / bundled 加载结果。

## 运行时范围

Skills 仅在 **general** lead agent 下加载；详见 [skills-persistence.md](skills-persistence.md)。

## 跨平台

macOS / Windows / Linux 使用相同路径约定（`~` 为用户主目录，`CODEX_HOME` 可覆盖 Codex 根目录）。
