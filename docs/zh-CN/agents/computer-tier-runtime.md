# Computer Agent 模型配置与运行时

> Computer Agent **不再有自己的模型档位**。模型与思考强度跟 general / coder / web_search
> 完全共用同一套配置：`agentModeLlm[agentId][mode]` + `agentDefaultModels[agentId]`。
> 旧的 Primary / Intermediate / Advanced 三档、以及过渡期的 `computerModel` /
> `computerProvider` / `computerThinkingBudget` 三个专属键均已删除。

---

## 一、配置位置

| 键 | 作用 |
|---|---|
| `agentModeLlm["computer"][mode]` | 每个性能模式（`fast` / `standard` / `expert`）一条：provider + model + 思考强度 |
| `agentDefaultModels["computer"]` | 单一默认模型（`agentModeLlm` 缺失时兜底） |
| `agentPerformanceModes["computer"]` | 默认用哪一档（`fast` / `standard` / `expert`） |

每条 `agentModeLlm` 记录是 `ComputerTierLlmConfig`：

| 字段 | 说明 |
|---|---|
| `providerId` / `model` | 这一档用哪个模型 |
| `thinkingIntensity` | 思考强度档：`off` / `low` / `medium` / `high` / `max` |
| `enableThinking` / `thinkingBudget` / `reasoningEffort` | 内部兜底字段，UI 不暴露数字 budget |

解析顺序（`chat_service/session_model.rs`）：

1. `agentModeLlm[agentId][mode]` — 当前性能模式有配置就用它；
2. `agentDefaultModels[agentId]` — 否则用该 agent 的默认模型；
3. 都没有 → 保持会话当前的 provider / model。

解析完成后 `apply_tier_thinking_to_round()` 把该档的思考强度写进本轮 settings。

---

## 二、代码结构

| 位置 | 作用 |
|---|---|
| `agents/computer/tier/mod.rs` | `ComputerModelConfig`（只剩 `verify_host_enabled` + `pipeline_llm`） |
| `agents/computer/state/mod.rs` | `effective_tier_config()` 只合并 pipeline 配置；`apply_pipeline_phase_settings()` 处理 Position / Verify 阶段 |
| `chat_service/session_model.rs` | 通用的模型 + 思考强度解析（所有 agent 共用） |
| `models/settings.rs` | `agentModeLlm` / `agentDefaultModels` / `agentPerformanceModes` |

`ComputerModelConfig` 不再持有模型或 thinking 预算：

```rust
pub struct ComputerModelConfig {
    pub verify_host_enabled: bool,
    pub pipeline_llm: ComputerPipelineLlmConfig,
}
```

> 结构体名 `ComputerModelConfig` / `ComputerTierRuntime` 保留未改；后者现在只装
> 「工具历史 + 放弃/锁定状态」，与档位无关。

---

## 三、会话运行时

`ComputerTierRuntime` 每会话一份，纯内存、不落盘：

| 字段 | 作用 |
|---|---|
| `histories: Vec<TierActionRecord>` | 最近桌面工具调用（上限 `MAX_TIER_HISTORY`），用于 `[Recent desktop tool calls]` |
| `task_error_streak` / `goal_fail_streak` / `locked_goal` | 同一目标连续失败计数与锁定 |
| `should_give_up` / `give_up_reference` | 达到 `GIVE_UP_THRESHOLD` 后请求用户指导；重置时归档失败摘要 |

这些机制与模型档位无关。

---

## 四、Pipeline 阶段模型（与主模型独立）

`computerPipelineLlm` 仍单独配置 Decision / Position / Verify 三个阶段：

| 阶段 | 模型来源 | 思考 |
|---|---|---|
| Decision | `computerPipelineModelDecision` | 跟随本轮主模型（`agentModeLlm` 已注入） |
| Position | `computerPipelineModelPosition` | `computerPipelineThinkingBudgetPosition`（默认 1024） |
| Verify | `computerPipelineModelVerify` | `computerPipelineThinkingBudgetVerify`（默认 256） |

`computerPipelineVerifyHost` 控制宿主 post-execute verify。

---

## 五、已删除内容

| 旧 | 现状 |
|---|---|
| `ComputerTier::{Primary,Intermediate,Advanced}` | 已删除 |
| `computerInitialTier` | 已删除 |
| `computerModelPrimary/Intermediate/Advanced` | 已删除 |
| `computerTierLlm`（三键 map） | 已删除 |
| `computerModel` / `computerProvider` / `computerThinkingBudget` | 已删除（过渡期键，并入 `agentModeLlm`） |
| `ComputerTierGuard` / `current_computer_tier()` / `tier_allows_index_tools()` | 已删除 |
| `tier_runtime_prompt_block` / `includes_cause_in_history` | 已删除 |
| `computerAdvancedPipeline` | 已删除（只写不读） |
| `ComputerRoundLlmOverrides` / `apply_round_settings` | 已删除（computer 走通用路径） |
| `*_for_tier` 系列函数 | 去掉 tier 参数 |

旧设置文件兼容性：`StoredSettings` 没有 `deny_unknown_fields`，旧 JSON 里的
`computerInitialTier` / `computerTierLlm` / `computerModel` 等键会被 serde 静默忽略，
不会导致加载失败；但这些键会永久留在文件里，需要显式清理。
