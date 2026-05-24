# Coder Agent 能力增强路线图（实现规划 · 待评估）

> **目的**：把「当前 coder 智能体」与「高质量编程助手」之间的差距，拆成**可立项、可估复杂度、可估收益**的具体方案，便于你做 **P0 / P1 / P2** 取舍。  
> **范围**：以 `pointer-app` 现有架构为准（`pointer-core` 工具链、`chat_service` 对话循环、`context_compression`、Vue/Tauri 客户端）。**本文不实现代码**，仅作方案与挂载点说明。

---

## 1. 读者与使用方式

- **产品 / 技术负责人**：看 [§3 总览表](#3-方案总览与优先级矩阵) 与 [§4 分阶段建议](#4-推荐分阶段路线图) 做取舍。  
- **实现者**：看 [§5 分项说明](#5-分项实现说明)（含 [§5.13](#513-对标参考cursor-内置子代理subagents)、[§5.14](#514-nexplore--nbash-开发计划与优先级)）与 [§6 代码挂载点](#6-与现有代码的挂载点索引) 拆任务与估人天。  
- **评估收益**：结合 [§7 度量与验收](#7-度量与验收建议) 定义「做完算不算变好」。

---

## 2. 当前基线（事实快照）

以下基于仓库当前形态归纳，用于对比「缺什么」。

| 维度 | 现状 |
|------|------|
| **Agent 身份** | `crates/pointer-core/src/agents/coder/AGENT.md`：澄清 → 探索 → 计划 → 实现 → 单测 → 集成检查 → 交付；含读盘纪律与超限应对。 |
| **通信注入** | `agents/coder/COMMUNICATION.md`：工作区、`file` 政策、JSON 写编示例、`task_board` 与交付约定。 |
| **工具白名单** | `AGENT.md` frontmatter：`file`、`skill`、`terminal`、`task_board`（无 `web` / 专用 `git` / `lsp` 等）。 |
| **读文件** | `tools/file.rs` + `tools/prompts/file.md`：`paths` 批读为对象数组，每项须含 `path`，可选 `lineStart`/`lineEnd`/`maxBytes`；根级同名字段为缺省；`maxBytes` 默认 256KiB/文件；批读 `maxTotalBytes` 默认 1MiB；超限截断/跳过与 `batchCapped` 等字段。 |
| **改文件** | `file:edit` 仅 **`edits`** 数组（1–32 项，每项 `path`+`oldString`+`newString`）；响应含 `files` / `batchPartialFailure` 等。 |
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
| N | **子 Agent 编排与 trace（多子代理、resume、后台）** | Core 架构 | XL | M | 消息与 trace 模型、UI | 复杂度高、调试难 |
| N‑Explore | **Explore 型子代理**：只读并行代码库探索（`file` / `grep` / 后续搜索能力），**摘要**回主会话；对标 Cursor **Explore**；细节见 [§5.14](#514-nexplore--nbash-开发计划与优先级) | Core + Prompt | L | H（大仓） | 最小子会话协议；**D** 稳定后收益更大 | 摘要丢关键路径、与主 Agent 重复探索 |
| N‑Bash | **Bash 型子代理**：长输出 **`terminal`** 在子会话执行，**摘要 + exit code** 回主会话；对标 Cursor **Bash**；细节见 [§5.14](#514-nexplore--nbash-开发计划与优先级) | Core + Prompt | M | H | 现有 `terminal` 工具；可选与 **G** 协同 | 摘要漏掉失败栈顶行、安全与审计边界 |

**优先级建议（仅作起点，可按你方约束调整）**

- **P0（高收益 / 中低复杂度）**：**B**（上下文可预期）、**G**（测失败可读）、**K**（技能包）、**I**（纯提示自审）。  
- **P1（高收益 / 中高复杂度）**：**C**（外部事实）、**D**（搜索增强）、**A**（模型策略）。  
- **P2（视场景）**：**E**（LSP）、**F**（git 工具）、**H**（lint 深度）、**L**（评测集）、**N‑Bash**、**N‑Explore**（子代理专项；**实施顺序**见 [§5.14](#514-nexplore--nbash-开发计划与优先级)）。  
- **P3（长期）**：**J**（自动 critique）、**M**（IDE 级体验）、**N**（完整多子代理编排与产品化 trace）。

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
- **D**：`file:grep` 已用 **grep-searcher / grep-regex + ignore**（与 ripgrep 同栈的库实现，尊重 `.gitignore`、默认跳过隐藏路径）；若仍要 CLI `rg`，再评估是否重复。
- **A**：coder  profile 绑定更强模型或更高 `max_tokens`（若提供商支持）。

### 阶段 3：深度与体验

- **E / F**：按语言栈落地符号或 git 只读工具。  
- **L**：评测集与 CI 门禁。  
- **N‑Bash / N‑Explore**：按 [§5.14](#514-nexplore--nbash-开发计划与优先级) 的 **P2** 与 **实施顺序** 落地（建议阶段 3 前半启动 **N‑Bash**，后半或并行启动 **N‑Explore**）。  
- **M / N**：产品级投入；完整 **N** 可与 **N‑Explore / N‑Bash** 共用同一套子会话协议，逐步从「单类子代理」演进到「多子代理编排」。

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
- **第一阶段（已实现）**：DashScope 托管联网搜索 — **`web_search`** 工具（原生 API + 来源列表）+ **`research`** 纯联网子智能体。见 [`web-search-tool.md`](../guides/web-search-tool.md)。  
- **后续**：只读 MCP 或内置 `http_get` 类工具（指定 URL 抓取）：URL 白名单、重定向限制、TLS、响应体上限、HTML→文本；缓存（URL+etag）；提示词强制「引用官方文档要点」。  
- **复杂度**：第一阶段 `M`；完整 HTTP/MCP 仍为 `L`（安全与合规占大头）。  
- **收益**：`H`（对外部库重的任务）。

### 5.4 D — 搜索 / 导航增强

- **目标**：超大仓库里 **更快定位**、更少误 grep。  
- **实现要点**：评估 `file:grep` 是否已够用；若上 `rg`，需统一 **根目录、忽略规则（.gitignore）、二进制跳过**；提示词写清何时用哪个。  
- **落地方案（草案）**：在现有库栈上补 **glob/type、`-F`、`-i`、与默认 `rg` 一致的 hidden、并行 / 文件上限** 等，见 [`file-grep-d-enhancement-proposal.md`](file-grep-d-enhancement-proposal.md)。  
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
- **本地搭建（不依赖先实现完整评测产品化）**：见 [`coder-agent-offline-eval-setup.md`](../guides/coder-agent-offline-eval-setup.md)（判分脚本、Docker、接 `server` HTTP/SSE、本机 LLM）。

### 5.12 M / N — IDE 与子 Agent

- **M**：显著降低「不敢用 agent 改代码」的心理成本。  
- **N**：完整 **多子代理编排、trace、resume/后台** 等产品级能力；适合与 **N‑Explore / N‑Bash** 分阶段演进（先单类子代理 MVP，再合并为统一 **N**）。  
- **复杂度**：`XL`。  
- **收益**：`H`（M）、`M`–`H`（N，视场景）。  
- **产品对标**：Cursor 将「探索 / 终端 / 浏览器」做成内置子智能体时的设计取舍，见 [§5.13](#513-对标参考cursor-内置子代理subagents)。**Explore / Bash 对应落地项**见 [§5.14](#514-nexplore--nbash-开发计划与优先级)。

### 5.13 对标参考：Cursor 内置子代理（Subagents）

以下摘自 Cursor 公开文档，用于 **方案 N、N‑Explore、N‑Bash** 立项时的「能力拆分与上下文隔离」参考，**不代表** pointer-app 必须同名或同实现。

| 内置子代理 | 官方定位（摘要） | 主要解决的问题 |
|------------|------------------|----------------|
| **Explore** | 在代码库中 **搜索与分析**；可用更快模型、并行多路检索 | 大范围探索产生大量中间结果，避免撑爆主对话上下文 |
| **Bash** | 串联执行 **Shell** | 终端输出冗长，隔离在子会话，父会话只收结论 |
| **Browser** | 通过 **MCP** 驱动浏览器 | DOM、截图等噪声在子会话消化，父会话收摘要 |

**机制要点**（与 N 相关）：

- **独立上下文**：子智能体各自占用上下文窗口；长研究/探索不挤占主会话（见 [Subagents](https://cursor.com/docs/subagents)）。  
- **并行**：可并行启动多个子智能体，分工作流（同上）。  
- **工具与提示**：文档写明各内置类型有 **tuned** 的 prompts 与 tool access；FAQ 亦述子智能体可 **继承父级工具（含 MCP）**，但以官方当前版本说明为准。  
- **搜索链路**：主流程外的「广撒网」探索常与 **语义搜索 + Instant Grep + 读文件** 组合（见 [Semantic & agentic search](https://cursor.com/docs/agent/tools/search)）。

**对 pointer-app 的映射思路（待评估）**：

- 若落地 **N‑Explore / N‑Bash** 或完整 **N**，可显式区分 **只读探索子循环**（多路 `file`/`grep`/后续 `code:search`）与 **主对话改码循环**，并约定 **回传摘要结构**（路径列表、结论段落、禁止整屏 dump）。  
- **长输出命令**（`terminal`）是否拆子会话，可类比 Bash 子代理的「日志隔离」收益，再权衡实现成本。  
- **浏览器 / 富交互验证** 若未来接入 MCP，可类比 Browser 子代理的「噪声不外溢」原则单独设计。

### 5.14 N‑Explore / N‑Bash 开发计划与优先级

将 Cursor 内置 **Explore**、**Bash** 映射为可立项的两条方案（矩阵 ID **N‑Explore**、**N‑Bash**），与 umbrella **N** 区分：**N** 偏「编排与基础设施完备」；**N‑Explore / N‑Bash** 可先 **MVP 单路径** 上线。

| 项 | **N‑Bash**（Bash 型） | **N‑Explore**（Explore 型） |
|----|------------------------|-----------------------------|
| **优先级** | **P2** | **P2** |
| **推荐实施顺序** | **① 先做**（范围相对收束：围绕 `terminal` 输出形态与子会话边界） | **② 后做**（并行多路读/搜 + 摘要协议，依赖与验证面更大） |
| **目标** | 编译/测试/构建等 **长日志** 不撑爆主会话；主会话只保留 **短摘要、exit code、可选尾部原文片段** | 大仓 **广撒网** 探索（多文件 `grep`、批读、路径列表）在子会话完成，主会话只收 **结论与关键路径** |
| **依赖** | 现有 `terminal`；若已做 **G**，摘要可与结构化失败信息对齐 | **B**（预算可预期）、**D**（搜索路径清晰）可降低「主从重复搜」；最小子会话/子 trace 数据结构 |
| **验收要点** | 主会话 tool 结果长度上限内可读；失败场景下 **stderr 顶行 / 首个 error** 不得被摘要规则静默丢弃（需显式策略） | 探索子会话可并行发起多路只读调用；回传含 **文件路径 + 一句话结论**，禁止默认整文件 dump |
| **风险** | 摘要过粗导致排障困难 | 与主 Agent 探索职责重叠、重复 token |

**与 umbrella N 的关系**：**N‑Bash** 与 **N‑Explore** 可共用同一套「子上下文 + 回传摘要」协议；待两条 MVP 稳定后，再收敛到 **N**（多子代理调度、后台、resume 等 **P3** 能力）。

---

## 6. 与现有代码的挂载点索引

便于实现时快速跳转（路径相对仓库根）。

| 模块 | 路径 | 说明 |
|------|------|------|
| Coder Agent 定义 | `crates/pointer-core/src/agents/coder/AGENT.md` | frontmatter、`accessPolicy` |
| Coder 通信 | `crates/pointer-core/src/agents/coder/COMMUNICATION.md` | 注入片段 |
| File 工具 | `crates/pointer-core/src/tools/file.rs`、`tools/prompts/file.md` | 读盘上限与行为 |
| Terminal 工具 | `crates/pointer-core/src/tools/terminal.rs` | **N‑Bash** 长输出与子会话摘要挂载点 |
| 对话循环 / 工具调度 | `crates/pointer-core/src/chat_service/`（`agent_post_stream.rs`、`agent_stream_round.rs`、`agent_tool_pass.rs`、`single_agent.rs`、`session_inner.rs`、`sub_agent.rs` 等） | 挂新工具、改 tool result 形态需协调 |
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
4. **成本**：每任务 token / 费用（若有日志，参见 [`llm-token-usage-logging.md`](../llm/llm-token-usage-logging.md)）。  
5. **用户修正率**：用户后续消息中「纠正 agent」的占比（若可统计）。

每项方案上线后，至少跟踪 **1、2、3** 两周再决定是否扩大投入。

---

## 8. 风险汇总

| 风险 | 缓解 |
|------|------|
| 子代理摘要丢关键信息（**N‑Bash / N‑Explore**） | 摘要规则保留 exit code、首段错误、用户可配置「回传原始尾部行数」；失败时降级为截断原文而非纯述 |
| 联网工具安全 | 白名单、禁止 file URL、响应大小上限、审计日志 |
| 压缩丢信息 | 提高保留轮数、关键 user 消息 pin、摘要失败显式提示 |
| 解析类工具脆弱 | 版本化 parser、失败降级为原始输出 |
| 技能内容腐烂 | 与仓库版本号或 tag 绑定、CI 检查技能内命令仍有效 |
| 模型变强后提示词冗余 | 定期 A/B 缩短 AGENT 长度，避免上下文浪费 |

---

## 9. 文档维护

- 实施过程中若新增「Coder 与全局共用」的约定，可回链到 [`agent-task-board-and-verification.md`](../internals/agent-task-board-and-verification.md) 等现有文档，避免重复矛盾。  
- 本文档建议在 **每个阶段结束时** 更新一次「已落地 ID + 实测指标」，作为阶段复盘附件。

---

**版本**：初稿（规划用）；已补充 §5.13（Cursor 内置子代理对标）、§5.14（N‑Explore / N‑Bash 与 **P2** 实施顺序）。  
**维护者**：实现负责人按阶段更新  
