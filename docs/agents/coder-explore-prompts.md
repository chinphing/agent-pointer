# Coder + Explore 提示词结构

Executable 主流程（**模型可见**）：**G1 / G2 / G3** + **Orient → Change → Check**（可回环）→ **Deliver**。

产品语言对照（**仅本文档，不 compose**）：

| 产品说法 | 模型 prompt |
|----------|-------------|
| 澄清 | Orient·意图 |
| 探索 | Orient·Locate |
| 执行 | Change |
| 验证 | Check |

## 目录

```
agents/_shared/exploration/     impact_scan, handoff_contract, trace_when, file_discipline
agents/coder/
  AGENT.md                      manifest + 短 identity
  COMMUNICATION.md              工作区、file_edit、read_lints、JSON 示例
  prompts/                      role, flow/routine, delegation, task_board, scenarios/*
  author/legacy_agent.md        旧正文归档（不加载）
agents/explore/
  AGENT.md                      manifest + mission 摘要
  COMMUNICATION.md              只读约束
  prompts/                      role, flow/*, deliverable, scenarios/*
  author/legacy_agent.md
```

## 采纳与舍弃

- **保留**：coder/explore 去重；Impact > Trace；explore handoff；G2 场景路由；gap（scope、语言、并行、compression）。
- **不写入 prompt**：严格四段顺序；顶级 Forward/Backward trace；coder 完整 impact 表；Task Board 改造（维持现有 Pointer 机制）。

## 组装（Rust）

- `load_builtin_agent` 对 `coder` / `explore` 用 `composed_system_body()` 替换 `AGENT.md` 正文。
- `compose_system_prompt(COMMUNICATION, body)` 不变。
- 千问 cache 顺序见 `docs/internals/llm-prompt-assembly-order.md`。

## G2 Scenario（explore `instruction` 内）

| Scenario | explore 要点 |
|----------|--------------|
| `narrow_confirm` | 不委派；lead 本地 1 grep + 1 read |
| `single_module_fix` | References + Readers + Tests |
| `cross_module_change` | lite Impact map + Surfaces |
| `architecture_explain` | Summary + Key files + Execution paths |
| `reachability_audit` | layer tags + negative greps |
| `spec_map` | References + Registration + Test&drift |
| `design_only` | Key files + Summary |
| `production_debug` | symptom 向后 Execution paths ≤5 hop |

修饰符：`Scenario: production_debug+cross_module`。

## 并行探索编排

- coder 在发起宽范围 explore 前，先按独立模块、层、包或平台入口划分未知项。
- 两个及以上互不依赖的只读范围，应在同一轮发起多个 explore 调用。
- 每个 explore 使用不重叠的范围，以及独立的 `taskId` 和标题。
- 紧耦合的端到端调用链、共享状态边界仍由单个 explore 追踪。
- coder 在进入 Change 前合并全部 handoff；冲突证据通过窄范围本地检查确认。

## Eval 断言（`agents/mod.rs` tests）

- explore prompt **不含** `## Forward trace` / `## Backward trace` 顶级标题。
- explore prompt **含** `G1` / `G2` / `trace_when` 或 Execution paths 规则。
- coder prompt **不含** `Change impact scan` 旧节标题；**含** `G1` / `G2` / `G3`。
- legacy 正文在 `author/legacy_agent.md`（A/B 对照，不加载）。

## Phase 5 可选（roadmap 注记）

- **Coder Skills**：按场景挂载 skill 包（未实现；见 `docs/design/coder-agent-capability-roadmap.md`）。
- **Scenario 修饰符**：`Scenario: production_debug+cross_module` 已在 explore router 与 delegation 文案中预留；运行时无额外解析。
- **语义搜索 / Bash 子代理**：workspace 仍以 `file_grep` 为主；Bash 子代理与 repo 语义索引为 roadmap 项，不在当前 compose 范围。
