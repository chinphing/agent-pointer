# Skills 使用

[English](../en/user/skills.md) | 简体中文

Pointer 通过 **Skills** 为对话注入可复用能力说明（翻译、文档处理、领域流程等）。

## 技能库

1. 打开 **Skills 技能库**
2. 勾选要启用的技能；状态保存在用户设置中，重启后仍生效
3. 新用户默认启用仓库内置技能（查找技能、开发环境、Skill 管理、Pointer 管理、浏览器自动化）。Office / PDF 技能需自行导入

启用后，**general** / **coder** 智能体会在对话中加载对应技能索引；模型需要时通过 `skill_read`（必填 `path`：说明用 `SKILL.md`，资源用相对路径）读取正文与资源。

只把技能文件夹放到 `~/.pointer/skills/` **不会自动启用**。需要在技能库勾选，或由助手执行导入并启用。

## 导入 zip

1. 技能库 → **导入 zip**
2. 每个 Skill 须为 **kebab-case 目录**，且含精确命名的 **`SKILL.md`**
3. 导入到 `~/.pointer/skills/`，并自动刷新列表

不支持 `skill.md`、`skill.json` 或根目录 `manifest.json`。

## 首次启动：从其它工具导入

首次启动时，若检测到 Codex / Claude / OpenClaw / Hermes 等目录中已有 Skill，会弹窗询问是否 **一键导入** 到 Pointer 用户库。拒绝或完成后不再重复询问。

## 修改技能内容

- 用户库 `~/.pointer/skills/` 中的技能可由 Agent 通过委派 **coder** 子智能体编辑
- 系统内置技能与应用 bundled 库为只读

## 格式与兼容

Skill 目录结构、frontmatter 字段、与 Codex / `.agents/skills` 的兼容规则见 **[`../developer/skills-compatibility.md`](../developer/skills-compatibility.md)**。

持久化与加载范围的实现细节见 **[`../developer/skills-persistence.md`](../developer/skills-persistence.md)**（面向集成与维护）。
