# Coder Agent 能力增强路线图（实现规划 · 待评估）

> **目的**：把「当前 coder 智能体」与「高质量编程助手」之间的差距，拆成**可立项、可估复杂度、可估收益**的具体方案，便于你做 **P0 / P1 / P2** 取舍。  
> **范围**：以 `pointer-app` 现有架构为准（`pointer-core` 工具链、`chat_service` 对话循环、`context_compression`、Vue/Tauri 客户端）。**本文不实现代码**，仅作方案与挂载点说明。

---

## 1. 读者与使用方式

- **产品 / 技术负责人**：看 [§3 总览表](#3-方案总览与优先级矩阵) 与 [§4 分阶段建议](#4-推荐分阶段路线图) 做取舍。  
- **实现者**：看 [§5 分项说明](#5-分项实现说明) 与 [§6 代码挂载点](#6-与现有代码的挂载点索引) 拆任务与估人天。  
- **评估收益**：结合 [§7 度量与验收](#7-度量与验收建议) 定义「做完算不算变好」。

---

## 2. 当前基线（事实快照）

以下基于仓库当前形态归纳，用于对比「缺什么」。

| 维度 | 现状 |
|------|------|
| **Agent 身份** | `crates/pointer-core/src/agents/coder/AGENT.md`：澄清 → 探索 → 计划 → 实现 → 单测 → 集成检查 → 交付；含读盘纪律与超限应对。 |
| **通信注入** | `agents/coder/COMMUNICATION.md`：工作区、`file` 政策、JSON 写编示例、`task_board` 与交付约定。 |
| **工具白名单** | `AGENT.md` frontmatter：`file`、`skill`、`terminal`、`task_board`（无 `web` / 专用 `git` / `lsp` 等）。 |
| **读文件** | `tools/file.rs` + `tools/prompts/file.md`：`paths` 批支持每元素为带 `path` 的对象以单独设置 `lineStart`/`lineEnd`/`maxBytes`（可与字符串路径混用）；`maxBytes` 默认 256KiB/文件；批读 `maxTotalBytes` 默认 1MiB；超限截断/跳过与 `batchCapped` 等字段。 |
| **改文件** | `file:edit` 支持单次 **`edits`** 批（≤32 项，顺序应用；`files`/`batchPartialFailure`）；单文件仍为 `path`+`oldString`+`newString`。 |
| **上下文** | `context_compression.rs`：超字符预算时可摘要前缀（依赖设置项）；非「无限上下文」。 |
| **技能** | `defaultSkillIds: []`，`allowSkills: []`：技能 harness 可用但未预置领域技能包。 |

**结论**：工作流与「少读、窄读、可验证」已在提示词与 `file` 工具层面对齐；差距主要在 **外部事实、大仓导航、验证纵深、可复用资产、评测闭环、模型与 IDE 体验**。

---

## 3. 方案总览与优先级矩阵

**复杂度**：`S` 小（≈0.5–2 人天）、`M` 中（≈3–10 人天）、`L` 大（≈2–6 周）、`XL` 特大（多迭代/架构级）。  
**收益**：`H` 高（明显少错任务 / 明显省 token 或时间）、`M` 中、`L` 低（边际改善）。  
**形态**：`Prompt` 仅提示词；`Config` 设置/默认项；`Core` Rust `pointer-core`；`Client` Vue/Tauri；`Content` 文档/技能内容生产。

| ID | 方案 | 形态 | 复杂度 | 收益 | 依赖 | 风险 |
|----|------|------|--------|------|------|------|
| A | **Coder 专用模型路由 / 温度 / max_tokens 策略** | Config + Core | S | M | 提供商 API、计费 | 成本上升 |
| B | **上下文：默认预算、压缩开关与「摘要失败」降级 UX** | Config + Client + Core | M | H | `context_compression`、`ModelSettings` | 摘要失真导致「丢需求」 |
| C | **外部文档 / 联网：Web 抓取或 MCP（只读）** | Core (+ MCP) + Prompt | L | H | 网络安全策略、超时、缓存 | SSRF、合规、不稳定 HTML |
| D | **仓库导航增强：`rg`/`git grep` 封装或专用 `code:search` 工具** | Core + Prompt | M | H | 与现有 `file:grep` 关系需设计 | 与 `file:grep` 能力重复或分叉 |
| E | **符号级导航：LSP 单次 query 或 `rust-analyzer`/`tsserver` CLI 包装** | Core + Prompt | L | H | 语言服务安装、跨平台 | 环境不一致、维护成本高 |
| F | **Git 一等工具：`git:status` / `git:diff` / `git:log`（只读）** | Core + Prompt | M | M | 工作区为 git 仓 | 非 git 工作区降级 |
| G | **测试结果结构化摘要（解析 `cargo test` / `vitest` 等）** | Core | M | H | 各栈输出格式 | 解析脆弱需持续修 |
| H | **Lint/Format 门禁模板 + 提示词「改完必跑」强化** | Prompt + 可选 Core | S–M | M | 仓库脚本约定 | 假阳性拖慢 |
| I | **自审（Self-review）提示：改完后对照 diff 与 AC** | Prompt | S | M | 无 | 模型仍可能敷衍 |
| J | **自动第二遍 LLM（critique 子调用）** | Core | L | M | 延迟、费用翻倍 | 性价比未必好 |
| K | **内置 Coder Skills（Rust/TS/Monorepo 发布检查等）** | Content + Core 注册 | M | H | `skills/` 规范 | 内容腐烂需版本化 |
| L | **离线评测集（golden tasks + 自动判分）** | 仓库外或 `scripts/` | L | H | CI 时间、维护 | 一次投入大 |
| M | **IDE：diff 预览、分块应用、回滚** | Client | XL | H | 与 `file:edit` 协同 | 产品工作量极大 |
| N | **子 Agent / 并行探索（大仓分片）** | Core 架构 | XL | M | 消息与 trace 模型 | 复杂度高、调试难 |

**优先级建议（仅作起点，可按你方约束调整）**

- **P0（高收益 / 中低复杂度）**：**B**（上下文可预期）、**G**（测失败可读）、**K**（技能包）、**I**（纯提示自审）。  
- **P1（高收益 / 中高复杂度）**：**C**（外部事实）、**D**（搜索增强）、**A**（模型策略）。  
- **P2（视场景）**：**E**（LSP）、**F**（git 工具）、**H**（lint 深度）、**L**（评测集）。  
- **P3（长期）**：**J**（自动 critique）、**M**（IDE 级体验）、**N**（子 Agent）。

---

## 4. 推荐分阶段路线图

### 阶段 0：评估与基线（不开发或极少开发）

- 选 **5–10 个**真实内部任务（改 bug、加小功能、跨 2–3 包重构），用**当前** coder 跑一遍，记录：轮数、失败类型（读不全 / 测没跑 / API 用错）、token 或耗时（若有）。  
- 输出一张「失败模式 → 对应方案 ID」表，验证上表 P0 是否与你数据一致。

### 阶段 1：「少错 + 省上下文」为主（约 1–2 个迭代）

- **B**：默认开启或引导开启合理 `context_budget_chars`；UI 说明压缩含义；摘要失败时的用户可见提示（若尚未完善）。  
- **I**：在 `AGENT.md` **Implement** 后增加固定小节「Self-review checklist」（3–8 条可勾选式短句）。  
- **K**：先做 **1 个**高 ROI 技能（例如「本仓库 Rust：`cargo test -p …` 范围约定」），验证 `skill` 加载与 coder 调用路径。  
- **G**：先做 **Rust `cargo test`** 输出解析 MVP（你们主栈），再扩展。

### 阶段 2：「对齐外部世界 + 大仓」为主

- **C**：MCP 或受限 HTTP fetch；白名单域、超时、响应体上限、Markdown 提取；提示词规定「改第三方 API 前必查」。  
- **D**：明确 `file:grep` 与「新搜索工具」边界；可选封装 `rg` 做 **路径/大小/二进制跳过** 与性能一致。  
- **A**：coder  profile 绑定更强模型或更高 `max_tokens`（若提供商支持）。

### 阶段 3：深度与体验

- **E / F**：按语言栈落地符号或 git 只读工具。  
- **L**：评测集与 CI 门禁。  
- **M / N**：产品级投入。

---

## 5. 分项实现说明

### 5.1 A — Coder 模型与解码策略

- **目标**：难任务更少「写到一半断气」、简单任务更省。  
- **实现要点**：`AgentDef` / profile 级默认 `max_tokens`、可选 `temperature`；或对话级覆盖。  
- **复杂度**：`S`（若已有设置模型字段）；若要做「按任务分类路由」则升为 `M`。  
- **收益**：`M`–`H`（取决于当前模型瓶颈是否在于输出长度或推理质量）。

### 5.2 B — 上下文预算与压缩

- **目标**：长会话下行为**可预测**，减少「突然丢前文」的客诉。  
- **实现要点**：梳理 `ModelSettings` 中与 `context_compression_enabled`、`context_budget_chars`、`context_keep_recent_user_turns` 的默认值与 UI 说明；可选「压缩前确认」或「仅 warn」。  
- **复杂度**：`M`（产品文案 + 边界测试）。  
- **收益**：`H`。

### 5.3 C — 外部文档 / 联网

- **目标**：减少「训练截止后 API 用错、版本错」。  
- **实现要点**：只读 MCP 或内置 `http_get` 类工具：URL 白名单、重定向限制、TLS、响应体上限、HTML→文本；缓存（URL+etag）；提示词强制「引用官方文档要点」。  
- **复杂度**：`L`（安全与合规占大头）。  
- **收益**：`H`（对外部库重的任务）。

### 5.4 D — 搜索 / 导航增强

- **目标**：超大仓库里 **更快定位**、更少误 grep。  
- **实现要点**：评估 `file:grep` 是否已够用；若上 `rg`，需统一 **根目录、忽略规则（.gitignore）、二进制跳过**；提示词写清何时用哪个。  
- **复杂度**：`M`。  
- **收益**：`H`（大仓）；`M`（中小仓）。

### 5.5 E — 符号 / LSP

- **目标**：「跳定义 / 找引用」准确率高于纯文本。  
- **实现要点**：短期可用 **语言专属 CLI**（如 `rust-analyzer` 的 `analysis-broker` 或项目内已有脚本）；长期再考虑常驻 LSP。  
- **复杂度**：`L`。  
- **收益**：`H`（复杂类型系统）；中小任务可为 `M`。

### 5.6 F — Git 只读工具

- **目标**：改前改后 **diff 范围可控**、减少漏提交文件。  
- **实现要点**：子命令固定为 `git status/diff/log`；路径参数校验；非 git 仓返回明确错误。  
- **复杂度**：`M`。  
- **收益**：`M`（工程习惯好）；若终端已熟练用 git 则边际下降。

### 5.7 G — 测试结果结构化

- **目标**：模型和人类一眼看到 **失败用例名与路径**，而不是整屏 stderr。  
- **实现要点**：`terminal` 执行后增加可选 **parser 插件**（按 command 前缀匹配 `cargo test` / `npm test`）；将摘要写入 tool result 前缀或单独字段（需模型协议兼容）。  
- **复杂度**：`M`。  
- **收益**：`H`。

### 5.8 H — Lint / Format 门禁

- **目标**：与「集成检查」章节一致且可执行。  
- **实现要点**：多数先 **Prompt** 固定「本仓库主命令」；若要做工具，则是包装 `npm run lint` 等并解析 exit code + 前 N 行。  
- **复杂度**：`S`–`M`。  
- **收益**：`M`。

### 5.9 I / J — 自审

- **I**：仅提示词，成本低，**建议先做**。  
- **J**：多一次 LLM，成本与延迟显著；适合 **高价值 PR** 或用户显式开启「深度审查模式」。  
- **复杂度**：`S`（I）、`L`（J，含 UI 开关与取消）。  
- **收益**：`M`。

### 5.10 K — Coder Skills

- **目标**：把「本仓库怎么做」从长 AGENT 挪到 **可版本化技能**，减少漂移。  
- **实现要点**：在 `skills/` 增加 `coder-rust-workspace` 等；`AgentDef.defaultSkillIds` 绑定；文档说明技能发现与调用约定（见 `tools/prompts/skill.md`）。  
- **复杂度**：`M`（内容 + 联调）。  
- **收益**：`H`（一致性）。

### 5.11 L — 离线评测集

- **目标**：改版、换模型、加工具后 **可回归**。  
- **实现要点**：固定输入消息 + 期望「可自动判分」条件（编译通过、测试通过、文件包含子串）；Docker 或裸机 runner；不强制与 CI 同一阶段。  
- **复杂度**：`L`。  
- **收益**：`H`（长期）；短期无直接用户感知。  
- **本地搭建（不依赖先实现完整评测产品化）**：见 [`docs/coder-agent-offline-eval-setup.md`](coder-agent-offline-eval-setup.md)（判分脚本、Docker、接 `server` HTTP/SSE、本机 LLM）。

### 5.12 M / N — IDE 与子 Agent

- **M**：显著降低「不敢用 agent 改代码」的心理成本。  
- **N**：适合 **超大探索任务**；需 supervisor 消息合并策略。  
- **复杂度**：`XL`。  
- **收益**：`H`（M）、`M`–`H`（N，视场景）。

---

## 6. 与现有代码的挂载点索引

便于实现时快速跳转（路径相对仓库根）。

| 模块 | 路径 | 说明 |
|------|------|------|
| Coder Agent 定义 | `crates/pointer-core/src/agents/coder/AGENT.md` | frontmatter、`accessPolicy` |
| Coder 通信 | `crates/pointer-core/src/agents/coder/COMMUNICATION.md` | 注入片段 |
| File 工具 | `crates/pointer-core/src/tools/file.rs`、`tools/prompts/file.md` | 读盘上限与行为 |
| 对话循环 / 工具调度 | `crates/pointer-core/src/chat_service.rs` | 挂新工具、改 tool result 形态需协调 |
| 上下文压缩 | `crates/pointer-core/src/context_compression.rs` | 预算与摘要 |
| 模型设置 | `crates/pointer-core/src/models.rs`（`ModelSettings` 等）、`storage` | 默认项与持久化 |
| 技能 | `crates/pointer-core/src/skills/`、`tools/skill.rs` | 新技能注册 |
| 内置工具注册 | `crates/pointer-core/src/tools/builtin.rs`（或等价入口） | 注册新 `ToolEntry` |
| 客户端 | `src/`（Vue）、Tauri | B/M 类 UX |

---

## 7. 度量与验收建议

避免「感觉变强了」无法论证：

1. **任务成功率**：阶段 0 固定任务集上，**通过 / 需人介入补救** 比例。  
2. **平均轮数**：完成同一任务所需 assistant 轮数（含工具）。  
3. **回归失败类型计数**：读错文件、未跑测、API 版本错、格式未跑等分类 tally。  
4. **成本**：每任务 token / 费用（若有日志，参见 `docs/llm-token-usage-logging.md`）。  
5. **用户修正率**：用户后续消息中「纠正 agent」的占比（若可统计）。

每项方案上线后，至少跟踪 **1、2、3** 两周再决定是否扩大投入。

---

## 8. 风险汇总

| 风险 | 缓解 |
|------|------|
| 联网工具安全 | 白名单、禁止 file URL、响应大小上限、审计日志 |
| 压缩丢信息 | 提高保留轮数、关键 user 消息 pin、摘要失败显式提示 |
| 解析类工具脆弱 | 版本化 parser、失败降级为原始输出 |
| 技能内容腐烂 | 与仓库版本号或 tag 绑定、CI 检查技能内命令仍有效 |
| 模型变强后提示词冗余 | 定期 A/B 缩短 AGENT 长度，避免上下文浪费 |

---

## 9. 文档维护

- 实施过程中若新增「Coder 与全局共用」的约定，可回链到 `docs/agent-task-board-and-verification.md` 等现有文档，避免重复矛盾。  
- 本文档建议在 **每个阶段结束时** 更新一次「已落地 ID + 实测指标」，作为阶段复盘附件。

---

**版本**：初稿（规划用）  
**维护者**：实现负责人按阶段更新  
