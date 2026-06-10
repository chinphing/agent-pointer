# Skills 启用状态与加载范围

## 目录与可变性（Hermes 对齐 + Pointer 扩展）

| 层级 | 路径 | 来源 | 可 patch / Curator |
|------|------|------|-------------------|
| **用户库** | `~/.pointer/skills/` | `skill_import`、外部一键导入、Agent 创建 | ✅（非 pinned） |
| **系统库** | `{data_dir}/PointerApp/skills/` | 应用内置 bundled 同步 | ❌（`.bundled_manifest` 保护） |

运行时加载顺序：**用户库优先**，同名 id 覆盖系统库。

系统库同步规则（与 Hermes 同型）：

- 安装/更新时写入 `{data_dir}/skills/.bundled_manifest`（`name:sha256`）
- 本地 hash 与 manifest **一致** → 可拉取新版本
- **不一致** → 视为已修改，跳过覆盖

用户库元数据：`~/.pointer/skills/.usage.json`（`agentCreated`、`pinned` 等）。

## 首次启动：外部 Skills 探测

**仅首次**（`~/.pointer/external_skills_probe_done` 不存在时）扫描：

| 来源 id | 目录 |
|---------|------|
| codex | `$CODEX_HOME/skills` 或 `~/.codex/skills` |
| claude | `~/.claude/skills` |
| openclaw | `~/.openclaw/skills` |
| hermes | `~/.hermes/skills` |

若存在尚未载入 Pointer 的 Skill，客户端/Web 弹窗询问是否 **一键导入** 到 `~/.pointer/skills/`。拒绝或导入完成后写入 marker，不再重复探测。

**注意**：运行时 **不再** 自动挂载 Codex/Hermes 等外部目录；仅通过首次导入或手动 `skill_import` 进入用户库。

## 全局持久化

已启用技能列表保存在 **`user_settings.json`** 的 `enabledSkillIds` 字段（`UserSettings`），跨会话、跨应用重启生效。APP 与 Web 通过 `updateUserSettings` 读写。

| 操作 | 行为 |
|------|------|
| 技能库勾选/取消 | `skills.toggle` → `saveUser({ enabledSkillIds })` |
| 应用启动 | `settings.load` 后 `skills.initEnabledFromUserSettings()` |
| `skill_import` + `auto_enable` | 后端写入 `user_settings.json` 并推送 `skills_updated` |
| 外部一键导入 | `import_external_skills` → 用户库 + `reload_meta` |

`Conversation.skillIds` 为历史字段，**不再**作为启用状态来源。

## Curator 与 Self-improvement

| 机制 | 范围 | 触发 |
|------|------|------|
| **Self-improvement review（P3）** | 仅 **mutable** 用户库 skill | `skillCreationNudgeInterval` |
| **Curator（P5）** | 仅 `is_curation_eligible` 用户库 skill | 空闲 + 周期 |

系统库 skill 在 API 中 `provenance: "system"`、`mutable: false`。

## 仅 general agent 加载技能

运行时规则（`pointer-core`）：

- **单智能体模式**：仅当 lead agent id 为 **`general`** 时，`build_plan` 注入技能索引与 `skill_*` 工具。
- **Supervisor 模式**：不加载技能。
- **子 Agent**：不加载技能。

前端发消息时：仅当 `agentMode === 'single'` 且 `leadAgentId === 'general'` 才传 `enabledSkillIds`。

## 内置技能

仓库 `skills/` 随应用打包；启动时同步到 **`{data_dir}/PointerApp/skills/`** 并登记 manifest。默认启用见 `DEFAULT_ENABLED_SKILL_IDS`。

## 实现入口

- `crates/pointer-core/src/skills/provenance.rs` — manifest、mutable 判定、patch guard
- `crates/pointer-core/src/skills/external.rs` — 双目录加载、bundled sync
- `crates/pointer-core/src/skills/external_probe.rs` — 首次外部探测与导入
- `crates/pointer-core/src/skills/curator.rs` — 用户库保洁
- `src/components/skills/ExternalSkillsImportModal.vue` — 首次导入弹窗
