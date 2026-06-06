# Skills 启用状态与加载范围

## 全局持久化

已启用技能列表保存在 **`user_settings.json`** 的 `enabledSkillIds` 字段（`UserSettings`），跨会话、跨应用重启生效。APP 与 Web 通过 `updateUserSettings` 读写。

技能包本体安装在 `{data_dir}/skills/`，与是否启用无关。

| 操作 | 行为 |
|------|------|
| 技能库勾选/取消 | `skills.toggle` → `saveUser({ enabledSkillIds })` |
| 应用启动 | `settings.load` 后 `skills.initEnabledFromUserSettings()` |
| `skill_import` + `auto_enable` | 后端写入 `user_settings.json` 并推送 `skills_updated` |

`Conversation.skillIds` 为历史字段，**不再**作为启用状态来源。

## 仅 general agent 加载技能

运行时规则（`pointer-core`）：

- **单智能体模式**：仅当 lead agent id 为 **`general`** 时，`build_plan` 注入技能索引与 `skill_*` 工具。
- **Supervisor 模式**：不加载技能。
- **子 Agent**（`explore`、`research`、`coder` 等）：不加载技能。
- **Coder** 等其它 worker 的 `AGENT.md` 不包含 `skill_load_instructions` / `skill_read_resource`。

前端发消息时：仅当 `agentMode === 'single'` 且 `leadAgentId === 'general'` 才传 `enabledSkillIds`；其余情况传空数组（与后端双重保险）。

## 实现入口

- `crates/pointer-core/src/models.rs` — `UserSettings.enabled_skill_ids`
- `crates/pointer-core/src/agents/mod.rs` — `agent_supports_skills` / `resolve_skill_ids`
- `src/stores/skills.ts` — 全局 toggle 持久化
- `src/stores/chat.ts` — `enabledSkillIdsForRequest()`
