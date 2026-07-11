# Computer Agent Tier Runtime — 三级升级核心逻辑

> 本文档记录 Computer Agent 的 Primary / Intermediate / Advanced 三级运行时：升级决策引擎、模型配置链、提示词加载链、放弃与锁定机制。

---

## 一、三层 Tier 架构

| Tier | 定位模式 | 默认模型 | thinking budget | 索引工具 | 截图 |
|------|---------|---------|-----------------|---------|------|
| Primary | index + coordinate | `qwen3.5-flash` | 2048 | ✅ 允许 | 2–3 张 |
| Intermediate | index + coordinate | `qwen3.5-plus` | 2048 | ✅ 允许 | 2–3 张 |
| Advanced | index + coordinate | `qwen3.6-plus` | 8192 | ✅ 允许 | 2–3 张 |

**提示词**：三档共用 Primary 的 `communication.md` + `loop.md`（`agents/mod.rs`）；仅 LLM 模型与 thinking budget 按 tier 切换。

代码：`crates/pointer-core/src/agents/computer/tier/mod.rs:50-54`

---

## 二、核心常量

代码：`crates/pointer-core/src/agents/computer/tier/mod.rs:29-46`

```rust
TIER_UPGRADE_THRESHOLD: u32 = 3;   // 自动升级阈值（verify fail > 3 次）
GOAL_LOCK_THRESHOLD: u32 = 3;      // [LOCKED GOAL] 锁定阈值（同一 goal fail > 3 次）
GIVE_UP_THRESHOLD: u32 = 5;        // 放弃阈值（repetition >= 5）
```

---

## 三、升级决策引擎 — `on_round_complete()`

### 3.1 触发路径

```
Agent 循环完成一轮
  → ComputerState::on_assistant_round_complete()    (state/mod.rs:654)
    → parse_tier_signal_from_sidecar_tool_calls()   解析 verify:report 的 sidecar 信号
    → backfill_newest_open_verify()                 回填到最末条 open 行（pending 不回填）
    → same_goal_repetition_count_in_history()       计算 host 端重复计数
    → on_round_complete() 仅 backfill 成功时调用    ★ 核心决策 (tier/mod.rs)
  → 检查 should_give_up                            放弃检查 (single_agent.rs:186 / sub_agent.rs:182)
```

### 3.2 决策逻辑（伪代码）

```
on_round_complete(config, parsed_verify, last_tool_goal, sidecar_rep, host_rep):
    
    if parsed_verify 为 None:
        return  // 没有 verify 信号，不做任何改变
    
    if step_result == "fail":
        tier_error_streak += 1                       // tier 级别 fail 计数
        goal_fail_streak += 1                        // 同 goal fail 计数
        
        if goal_fail_streak > GOAL_LOCK_THRESHOLD (3):
            locked_goal = Some(当前 goal)             // ★ 锁定当前 goal
            task_error_streak = goal_fail_streak
    
    if step_result == "pass":
        tier_error_streak = 0                        // ★ 全部清零
        should_give_up = false
        locked_goal = None
        goal_fail_streak = 0
        current_tier = config.initial_tier           // ★ 回到初始 tier
        // 完成！退出升级逻辑
    
    if step_result != "pass":
        effective_rep = max(sidecar_rep, host_rep)
        
        // ★ 自动升级判定
        should_upgrade = 
            (effective_rep > 0 && effective_rep > TIER_UPGRADE_THRESHOLD)
            || (effective_rep == 0 && tier_error_streak > TIER_UPGRADE_THRESHOLD)
        
        if should_upgrade && config.auto_upgrade:
            current_tier = current_tier.bump()       // Primary→Intermediate→Advanced
            tier_error_streak = 0
        
        // ★ 放弃判定
        if effective_rep >= GIVE_UP_THRESHOLD:
            should_give_up = true
```

### 3.3 升级路径图

```
                    verify fail > 3            verify fail > 3
   Primary ──────────────────────→ Intermediate ──────────────────────→ Advanced
      ↑                                                                   │
      │                                                              (封顶)
      │                                                                    │
      └──────────── verify pass = 重置到 initial_tier ────────────────────┘
```

- **自动升级条件**：`auto_upgrade=true`（默认）且 `tier_error_streak > 3` 或 `effective_rep > 3`
- **重置条件**：任何一步 `verify:report` 返回 `pass` → 立即回到 `config.initial_tier`
- **Advanced 封顶**：`Advanced.bump() == Advanced`，到达 Advanced 后不再升级

---

## 四、信号来源

Tier 系统的输入有两个信号源，取两者最大值：

### 4.1 Sidecar 信号（优先）

模型通过 `verify:report` sidecar 工具报告。解析器：`parse_tier_signal_from_sidecar_tool_calls()` (`tier/mod.rs:593`)

```json
{
  "action_result": "pass|fail|pending|n/a",
  "repetition_count": 3,
  "failure_cause": "precision_miss|wrong_operation"  // 仅 fail 时需要
}
```

### 4.2 Host 端计数

`same_goal_repetition_count_in_history()` 遍历 tier 历史记录，统计同一 goal 的 verify fail 次数。此计数独立于模型自报的 sidecar 信号——当模型未正确报告时作为兜底。

### 4.3 Effective repetition

取两者 max：`max(sidecar_rep, host_rep)`

### 4.4 History 行 verify 后缀（简化状态机）

`[Recent desktop tool calls]` 每行末尾由 Host 维护 `verify:` 后缀：

| 展示 | 含义 |
|------|------|
| `verify: verifying` | 最末 open 行，待本回合 Verify + `verify_report` |
| `verify: verified - pass` / `verified - wrong_operation` / `verified - precision_miss` / `verified - n/a` | 已验过关账 |
| `verify: skipped` | **从未验过**，被新 desktop action 越过或 inject 清理；不计 fail streak |

**Host 规则（`tier/mod.rs`）：**

- **Verify / backfill 目标**：恒为最末条 `verify_result = None` 的行（`rposition`）。
- **`push_action` 前**：所有 open 行 → `skipped`，再 push 新行（`verifying`）。
- **`[CUR_SCREEN]` inject / `recent_actions_prompt_block` 前**：`auto_close_stale_open_rows` — 多行 open 时保留**最末 open**，更旧行 → `skipped`。
- **`action_result=pending`**：sidecar 接受但不 backfill；`on_round_complete` 不触发。
- **重复 `verify_report`**（最末行已终态）：忽略 + warn。

提示词要求模型：最末行 `verified - *` 或 `skipped` 时跳过 Verify，不再调用 `verify_report`。

---

## 五、状态机关键字段

代码：`tier/mod.rs:205-218`，`ComputerTierRuntime` 结构体

```
current_tier          : ComputerTier    当前 tier (Primary/Intermediate/Advanced)
tier_error_streak     : u32             tier 级别连续 verify fail 计数
task_error_streak     : u32             锁定 goal 后的 fail 计数
locked_goal           : Option<LockedGoal> 当前锁定的 goal
goal_fail_fingerprint : String          当前 goal 的指纹（大小写+空白规范化）
goal_fail_streak      : u32             当前 goal 的连续 fail 计数
should_give_up        : bool            是否应当放弃
histories             : HashMap<ComputerTier, Vec<TierActionRecord>> 每个 tier 独立历史（最多 10 条）
```

状态隔离：每个 tier 维护独立的历史记录，升级后新 tier 的历史从空开始。

---

## 六、模型配置：三条数据源逐层合并

### 6.1 硬编码默认值

代码：`tier/mod.rs:17-24`

```rust
DEFAULT_MODEL_PRIMARY       = "qwen3.5-flash"
DEFAULT_MODEL_INTERMEDIATE  = "qwen3.5-plus"
DEFAULT_MODEL_ADVANCED      = "qwen3.6-plus"
PRIMARY_INTERMEDIATE_THINKING_BUDGET = 2048
ADVANCED_THINKING_BUDGET            = 8192
```

### 6.2 from_agent_registry() — AGENT.md config 表

代码：`tier/mod.rs:107-129`

```yaml
# AGENT.md frontmatter
config:
  computerAutoUpgrade: "true"
  computerInitialTier: "intermediate"
  computerModelPrimary: "qwen3.5-flash"
  computerModelIntermediate: "qwen3.5-plus"
  computerModelAdvanced: "qwen3.6-plus"
```

支持的 key: `computerAutoUpgrade`, `computerInitialTier`, `computerModelPrimary/Intermediate/Advanced`

### 6.3 apply_app_settings() — 用户设置覆盖

代码：`tier/mod.rs:140-146`

仅**初始 tier** 可被用户设置覆盖（`computerInitialTier` 非空时生效）。

### 6.4 apply_platform_tier_llm() — 平台端覆盖（最灵活）

代码：`tier/mod.rs:149-168`

平台下发 `computerTierLlm` map:

```json
{
  "primary":      { "model": "qwen3.5-flash", "enable_thinking": true,  "thinking_budget": 2048 },
  "intermediate": { "model": "qwen3.5-plus",  "enable_thinking": true,  "thinking_budget": 2048 },
  "advanced":     { "model": "qwen3.6-plus",  "enable_thinking": true,  "thinking_budget": 8192 }
}
```

平台配置完全覆盖默认模型名，`enable_thinking` 和 `thinking_budget` 用于运行时动态选择。

### 6.5 合并链路

```
① Agent 清单 (AGENT.md config)
    ↓ from_agent_registry()
② ComputerTierConfig::default()   ← 硬编码默认值
    ↓ apply_app_settings()         ← 用户设置覆盖 (computerInitialTier)
    ↓ apply_platform_tier_llm()    ← 平台 tier LLM 配置覆盖 (computerTierLlm)
③ effective_tier_config()          ★ 最终生效配置 (state/mod.rs:137)
```

---

## 七、每轮模型选择：`ComputerRoundLlmOverrides`

### 7.1 入口

`single_agent.rs:107` 每轮循环开始时：

```rust
let round_settings = if lead_profile == AgentProfile::Computer {
    state.computer_state.apply_round_settings(conversation_id, settings)
} else {
    settings.clone()
};
```

### 7.2 apply_round_settings()

代码：`state/mod.rs:290-299`

```rust
pub fn apply_round_settings(&self, conversation_id: &str, settings: &ModelSettings) -> ModelSettings {
    let o = self.round_llm_overrides(conversation_id);    // 从会话取当前 tier → 生成 overrides
    let mut s = settings.clone();
    s.model = o.model;                                     // ★ 覆盖模型名
    s.round_enable_thinking = Some(o.enable_thinking);     // ★ 覆盖 thinking 开关
    s.round_thinking_budget = o.thinking_budget;           // ★ 覆盖 thinking budget
    s
}
```

### 7.3 for_tier() — 按 tier 生成 overrides

代码：`tier/mod.rs:787-805`

```rust
pub fn for_tier(tier: ComputerTier, config: &ComputerTierConfig) -> Self {
    // 1. 优先查平台端 tier_llm map (contains enable_thinking + thinking_budget 覆盖)
    if let Some(t) = config.tier_llm.get(tier.label()) {
        return Self { model: t.model.clone(), enable_thinking: t.enable_thinking, thinking_budget: t.thinking_budget };
    }
    // 2. 无平台覆盖时，fallback 到硬编码默认
    match tier {
        Primary     → model_primary      + thinking_budget=2048
        Intermediate→ model_intermediate + thinking_budget=2048
        Advanced    → model_advanced     + thinking_budget=8192
    }
}
```

**关键点**：每一轮循环都通过 `round_llm_overrides()` 动态读取当前会话的 tier，所以 tier 自动升级后下一轮立即生效新模型。

---

## 八、提示词加载

### 8.1 内建 agent：compile-time `include_str!`

代码：`agents/mod.rs`，编译时嵌入 Primary 提示词（**所有 tier 共用**）：

```rust
const COMPUTER_COMMUNICATION_PRIMARY  = include_str!("computer/prompts/tiers/primary/communication.md");
const COMPUTER_AGENT_PRIMARY          = include_str!("computer/prompts/tiers/primary/loop.md");
const COMPUTER_OS_PROMPT_MACOS        = include_str!("computer/prompts/os/macos.md");
```

`tiers/advanced/` 与 `tiers/intermediate/` 下的文件**不参与**运行时加载（仅保留作历史/撰写参考）。

### 8.2 外部 agent 目录：文件系统加载

`load_external_computer_communication()` (`agents/mod.rs`)，优先级：

1. `prompts/tiers/primary/communication.md`
2. `COMMUNICATION.md`（兜底）

### 8.3 按 tier 选择

代码：`agents/mod.rs`

| 函数 | Primary | Intermediate | Advanced |
|------|---------|-------------|----------|
| `computer_communication_for_tier()` | `primary/communication.md` + OS + disabled controls | 同 Primary | 同 Primary |
| `computer_agent_body_for_tier()` | `primary/loop.md` | 同 Primary | 同 Primary |

**Tier 仅影响模型**：`ComputerRoundLlmOverrides::for_tier()` 选择 `computerModelPrimary/Intermediate/Advanced` 与 thinking budget；提示词与 `[CUR_SCREEN]` 截图布局（2–3 张）三档一致。

### 8.4 每轮 System Prompt 组装

代码：`chat_service/prompts.rs`（`push_agent_role_cacheable_prompts`）

```rust
if profile == AgentProfile::Computer {
    let tier = computer_state.tier_for_conversation(conversation_id);  // ★ 读当前 tier（仅用于模型）
    let comm = computer_communication_for_tier(tier);   // ★ 恒为 Primary communication
    let body = computer_agent_body_for_tier(tier);      // ★ 恒为 Primary loop
    let merged = format!("{comm}\n\n---\n\n{body}");
    cacheable.push(expand_agent_prompt_placeholders(&merged, &session_vars));
}
```

最终 system prompt 完整结构：

```
┌─────────────────────────────────────────────┐
│ 1. COMMUNICATION_PUBLIC.md（共享规则）        │
├─────────────────────────────────────────────┤
│ 2. computer_communication_for_tier(tier)     │
│    ├── communication.md（Primary，全 tier）   │
│    ├── ui_disabled_controls.md               │
│    └── OS prompt (macOS/Windows/Linux)       │
├─────────────────────────────────────────────┤
│ 3. computer_agent_body_for_tier(tier)        │
│    └── loop.md（Primary，全 tier）             │
├─────────────────────────────────────────────┤
│ 4. tools_system_appendix                     │
├─────────────────────────────────────────────┤
│ 5. [Environment] (OS + 日期 + 工具列表)       │
├─────────────────────────────────────────────┤
│ 6. 动态注入 (dynamic block)                   │
│    ├── tier_runtime_prompt_block             │
│    ├── locked_goal_dynamic_block             │
│    ├── recent_actions_prompt_block            │
│    └── 扩展钩子                               │
└─────────────────────────────────────────────┘
```

---

## 九、`[LOCKED GOAL]` 机制

当同一个 goal 连续 fail **超过 3 次** (`GOAL_LOCK_THRESHOLD`)：

1. Goal 被锁定，指纹存入 `locked_goal` 字段
2. 通过 `tier_dynamic` 扩展 hook 注入 `[LOCKED GOAL]` 动态提示到 system prompt（`extension_hooks/tier_dynamic.rs:27-36`）
3. 模型被告知：在 Verify 确认当前 goal 完成前，不得切换目标
4. 仅当该 goal 的 verify pass 后才解锁

---

## 十、放弃机制 (`GIVE_UP_THRESHOLD = 5`)

当 `effective_repetition >= 5`：
- `should_give_up` 设为 `true`
- `effective_repetition = max(sidecar, host)`，但当 `sidecar > host` 时以 **host** 为准（sidecar 可能误计 `[Prior attempt — give up reference]` 段）
- 外层循环检测到该标志后抛出错误：

> "当前任务已尽力但仍无法完成（重复操作达到 5 次），请提供进一步指导。"

检测位置：
- `single_agent.rs:186` — 单 agent 循环
- `sub_agent.rs:182` — 子 agent 循环

### 用户新消息时的指导重置

用户发送新消息后、进入 agent 循环前，调用 `reset_for_new_user_guidance(initial_tier)`（`single_agent.rs` / `sub_agent.rs`）：

| 字段 | 重置行为 |
|------|----------|
| `should_give_up` | `false` |
| `tier_error_streak` | `0` |
| `task_error_streak` | `0` |
| `goal_fail_streak` / `goal_fail_fingerprint` | 清零 |
| `locked_goal` | `None` |
| `last_executed_goal` | `None` |
| `current_tier` | 恢复为 `initial_tier` |
| 各 tier `histories` | 清空 |

清空前，将失败操作摘要写入 `give_up_reference`，并在 `[Recent desktop tool calls]` 中以 **`[Prior attempt — give up reference]`** 段展示。该段：

- 供 **Next** 参考，避免重复相同 tool+target 组合
- **不参与** `same_goal_repetition_count_in_history` 计数
- **不可** 再次 verify / verify_report

---

## 十一、运行时全链路时序

```
用户发送消息
    ↓
run_single_agent_loop()  [每一轮]
    ↓
┌─ 1. tier_for_conversation(conv_id)              ← 读取 current_tier
├─ 2. positioning_mode_for_tier(tier)             ← 工具文档（全 tier 相同）
├─ 3. prepare_single_agent_round_prompts()        ← Primary prompt + 动态注入
├─ 4. apply_round_settings(conv_id, settings)     ← 按 tier 选模型 + thinking budget
├─ 5. run_provider_stream_round(...)              ← 发送 LLM 请求
│      ↓ LLM 返回 assistant message + tool_calls
├─ 6. 执行 tool_calls（如 mouse / hotkey / verify:report）
├─ 7. on_assistant_round_complete()
│      ├── parse_tier_signal_from_sidecar_tool_calls()
│      ├── same_goal_repetition_count_in_history()
│      ├── tier_runtime.on_round_complete()
│      │    ├── fail → tier_error_streak++; goal_fail_streak++
│      │    ├── fail>3 → locked_goal = Some(...)
│      │    ├── fail>3 && auto_upgrade → current_tier.bump()
│      │    ├── repetition>=5 → should_give_up = true
│      │    └── pass → 全部重置，current_tier = initial_tier
│      └── if should_give_up → 退出循环
└─ 回到步骤 1（下一轮用新 tier 的模型；提示词不变）
```

---

## 十二、关键文件索引

| 文件 | 职责 | 
|------|------|
| `crates/pointer-core/src/agents/computer/tier/mod.rs` | **核心**：tier 枚举、配置 merge、升级决策、ComputerRoundLlmOverrides、ComputerTierRuntime 状态机、unit tests（1158 行） |
| `crates/pointer-core/src/agents/computer/state/mod.rs` | ComputerState：apply_round_settings()、tier_for_conversation()、effective_tier_config()、on_assistant_round_complete()、tier_runtime_prompt_block()（839 行） |
| `crates/pointer-core/src/agents/mod.rs` | Agent 加载：computer_communication_for_tier()、computer_agent_body_for_tier()、include_str! 编译期嵌入（1189 行） |
| `crates/pointer-core/src/chat_service/single_agent.rs` | Agent 循环：每轮调用 tier_for_conversation + apply_round_settings，give_up 检测（272 行） |
| `crates/pointer-core/src/chat_service/single_agent_prompt.rs` | 每轮 prompt 组装：按 tier 选择 communication + loop body 并拼接（136 行） |
| `crates/pointer-core/src/agents/computer/tools/tool_prompts.rs` | 工具文档按 tier 的 positioning mode 选择 index 版或 coordinate 版 |
| `crates/pointer-core/src/agents/computer/tools/tool_tier_signal.rs` | Sidecar 工具：验证 verify:report 信号参数 |
| `crates/pointer-core/src/agents/computer/extension_hooks/tier_dynamic.rs` | 扩展钩子：注入 [LOCKED GOAL] 到 system prompt |
| `crates/pointer-core/src/chat_service/sub_agent.rs` | 子 agent 循环：give_up 检测 |
| `crates/pointer-core/src/agents/computer/prompts/tiers/primary/` | **全 tier 运行时 prompt**（communication.md + loop.md） |
| `crates/pointer-core/src/agents/computer/prompts/tiers/advanced/` | 遗留/撰写参考（运行时未加载） |
| `crates/pointer-core/src/agents/computer/prompts/os/` | OS 特定 prompt（macOS/Windows/Linux） |

---

## 十三、测试覆盖

`tier/mod.rs` 包含完整的单元测试（`#[cfg(test)]` 块），覆盖：
- Fingerprint 大小写和空格规范化
- `parse_verify_from_thoughts` 提取 step/cause
- 历史行格式化（含/不含 cause）
- 4 次 fail 后自动升级：Primary → Intermediate
- Primary tier 历史隐藏 cause
- `same_goal_repetition_count` 仅计数 fail
- Runtime block 展示策略提示
- `on_round_complete` give_up 设置（sidecar 和 host 两个路径）
- Verify pass 清除 give_up
- `reset_for_new_user_guidance`（含 give_up_reference 归档）
- Thinking budget 配置
- `apply_app_settings` 覆盖初始 tier
- Locked goal label 隔离
- Sidecar 信号解析（取最后一个 signal）
