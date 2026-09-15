# Skills 启用状态与加载范围

## 目录与可变性（Hermes 对齐 + Pointer 扩展）

| 层级 | 路径 | 来源 | 修改方式 |
|------|------|------|----------|
| **用户库** | `~/.pointer/skills/` | `skill_import`、外部一键导入、coder 子 agent 编辑 | **`run_subagent` → coder**（`file_*`）；**`skill_import`** 仅安装 |
| **兼容库** | `~/.agents/skills/` | Codex / Agent 标准目录（只读加载） | ❌（`skill_import` 复制到用户库后可由 coder 改） |
| **系统库** | `{data_dir}/PointerApp/skills/` | 应用内置 bundled 同步 | ❌（`.bundled_manifest` 保护） |

运行时加载顺序：**用户库** → **`~/.agents/skills`** → **系统库**；同名 id 以先扫描到的为准。

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

## 唯一启用状态：`agentSkillOverrides`

已启用技能按 agent 保存在 **`user_settings.json`** 的 `agentSkillOverrides`（`agentId → skill ids`）。

| 操作 | 行为 |
|------|------|
| 技能库勾选/取消 | `skills.toggleForAgent` → `saveUser({ agentSkillOverrides })` |
| 应用启动 | `settings.load` 后 `skills.initEnabledFromUserSettings()`（仅迁移遗留字段） |
| `skill_import` + `auto_enable` | 写入当前 lead 的 `agentSkillOverrides[leadId]` 并推送 `skills_updated` |
| 外部一键导入 | `import_external_skills` → 用户库 + `reload_meta` |

遗留字段 `enabledSkillIds` **不再参与运行时 resolve**；若存在且尚无 `general` override，启动时迁移到 `agentSkillOverrides.general`。

`Conversation.skillIds` 为历史字段，**不再**作为启用状态来源。

## 磁盘有 ≠ 已启用

| 层 | 位置 | 作用 | 谁写入 |
|----|------|------|--------|
| 文件 | `~/.pointer/skills/{name}/` | `skill_read` 按磁盘解析（不看启用清单） | coder `file_*`，或 `skill_import` |
| 启用清单 | `agentSkillOverrides[leadId]` | 进入 `<available_skills>`、自动匹配 | `skill_import(auto_enable)` 或设置页勾选 |

两层都要有才会被自动匹配。zip 重导入会覆盖用户库副本并计入 `imported`。override 不做存在性校验，未安装的 id 在索引里标「未安装」。

## 运行时解析（唯一链路）

```
effective = agentSkillOverrides[agentId] ?? defaultSkillIds
effective = merge missing bundled ids from defaultSkillIds
```

`accessPolicy.allowSkills` / `denySkills` 为遗留字段，**运行时不再过滤**。技能边界只由 `defaultSkillIds` + 用户 override 决定。

入口（APP / Web / IM / Cron / Webhook）**不携带** skill 列表；`run_chat` 在 overrides 为空时从 `user_settings.agentSkillOverrides` 加载。

- **单智能体**：lead 为 **`general`** / **`coder`** 时按上式注入。
  **general** 可 **`file_*`** + **`skill_import`**；何时本地写、何时
  **`run_subagent(coder)`** → 见 `run_subagent` 工具提示词 **`coder`** 小节
  （唯一来源）。**coder** 仅 **`skill_read`** + **`file_*`**。
- **Supervisor**：不加载技能。
- **子 Agent**：**coder** 用自身 `defaultSkillIds`（经同一链路）；**self fork** 用 `inheritsFromParent`（父已解析列表）。其他子 Agent 通常不加载 skill。

升级补全：resolve 时把 agent `defaultSkillIds` 中仍属 bundled 的 id 补进 stale override（例如旧 coder override 漏掉 **`skill-manager`**）。前端 `ensureSystemSkillsEnabled` 只做同逻辑的 **持久化**，让设置页勾选与运行时一致。

## Curator 与 Self-improvement

| 机制 | 范围 | 触发 |
|------|------|------|
| **Self-improvement review（P3）** | 记忆 / 用户画像 | `memoryNudgeInterval`（skill 自动改写已禁用） |
| **Curator（P5）** | 用户库 stale 标记 / 归档 | 周期（无 LLM 改写） |

系统库 skill 在 API 中 `provenance: "system"`、`mutable: false`。`.agents/skills` 来源为 `provenance: "external"`、`mutable: false`。

## 内置技能

仓库 `skills/` 随应用打包；启动时同步到 **`{data_dir}/PointerApp/skills/`** 并登记 manifest。

**Agent 默认**：见各 agent `AGENT.md` 的 `defaultSkillIds`（与 bundled 对齐）。`general` 默认包含全部内置 skill。

**Office 技能（docx / xlsx / pptx / pdf）** 直接来自上游
[anthropics/skills](https://github.com/anthropics/skills)（含 `SKILL.md` 与 `scripts/`），
不要在本仓库手写精简版。更新时运行：

```bash
./scripts/sync-anthropic-office-skills.sh
```

## 实现入口

- `crates/pointer-core/src/agents/mod.rs` — `resolve_skill_ids` / `sub_agent_skill_ids`
- `crates/pointer-core/src/chat_service/session.rs` — 从 settings 加载 overrides
- `crates/pointer-core/src/skills/provenance.rs` — manifest、mutable 判定
- `crates/pointer-core/src/skills/external.rs` — 双目录加载、bundled sync
- `crates/pointer-core/src/skills/external_probe.rs` — 首次外部探测与导入
- `crates/pointer-core/src/skills/curator.rs` — 用户库保洁
- `src/components/skills/ExternalSkillsImportModal.vue` — 首次导入弹窗
