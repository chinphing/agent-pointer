# `file:grep` 增强方案（对齐路线图 D / 默认 `rg` 行为）

> **状态**：设计草案，供评审与估人天。  
> **范围**：在现有 `grep_searcher` + `grep_regex` + `ignore::WalkBuilder` 栈上增量演进；**不**引入本机 `rg` 子进程（可与「封装 rg」路线二选一或后置）。

---

## 1. 目标与非目标

### 1.1 目标

| 目标 | 说明 |
|------|------|
| **Glob / 类型过滤** | 缩小搜索面：等价于 `rg -g '*.rs'` / 常见 `--type` 用例，减少误匹配与 token。 |
| **`-F`（固定字符串）** | 用户字面量（含 `.`、`*` 等）不误触正则元字符；可用 `grep_regex::RegexMatcherBuilder::fixed_strings(true)`。 |
| **`-i`（大小写不敏感）** | 显式 JSON 开关；实现为 `case_insensitive(true)`，避免模型手写 `(?i)` 漏写。 |
| **Hidden 默认与 `rg` 一致** | 默认 **不** 搜索隐藏路径（点文件 / 点目录）；可选 `includeHidden: true` 覆盖。 |
| **吞吐** | 在 **单文件大小上限** 与 **maxResults** 仍受控的前提下，通过 **并行搜文件** 或 **适度调高单文件上限** 改善大仓体验。 |

### 1.2 非目标（本方案不做或后置）

- **多行跨行匹配**（`rg -U`）：仍保持行导向，避免与当前 `Searcher` 上下文语义纠缠。  
- **PCRE2 / 反引引用**：仍用 Rust `regex` 方言（与当前栈一致）。  
- **`.rgignore` / 任意 `--ignore-file`**：可后续用 `WalkBuilder` 的 API 扩展；本方案先做 **glob include**。  
- **语义搜索**：属索引 / 向量能力，不归本方案。

---

## 2. JSON API 设计（向后兼容）

在现有 `file:grep` 的 `tool_args` 上 **仅增加可选字段**；未传则行为尽量与「修正后的默认 `rg`」对齐（见 §3）。

| 字段 | 类型 | 默认 | 含义 |
|------|------|------|------|
| `includeGlobs` | `string[]` | 未设置 = 不额外限制 | 路径须 **至少匹配其中一条** glob（相对 **本次搜索根** `path` 或 workspace，与 `file:glob` 同一套相对语义）。多条为 **OR**。 |
| `excludeGlobs` | `string[]` | `[]` | 匹配则 **排除**（在 include 之后应用）。 |
| `fileTypes` | `string[]` | `[]` | 预置别名，展开为 **includeGlobs**（见 §4）；与 `includeGlobs` 同时存在时：**并集**再算 exclude。 |
| `fixedString` | `boolean` | `false` | `true` 时 `pattern` 按字面量匹配（`-F`）。 |
| `ignoreCase` | `boolean` | `false` | `true` 时忽略大小写（`-i`）。 |
| `includeHidden` | `boolean` | `false` | `true` 时搜索隐藏路径；默认 **`false`** 对齐 `rg`。 |

**上限与兼容**

- `pattern` 长度、 `maxResults`、`maxDepth`、`contextLines` 等 **保持现有 clamp**；若放宽单文件字节上限，见 §6。  
- **`path` 必填**（2026-06 已落地）：缺省不再从 workspace 根全盘 walk；仅当显式传 `path: "."` 时允许全仓搜索。  
- 响应 JSON 结构不变（`results` / `truncated` / `count`），必要时可增加 **`filesSkipped`**（因超 2MiB、二进制、glob 排除等计数）便于可观测性（可选，P1）。

---

## 3. Hidden 默认（行为变更）

- **现状**：`WalkBuilder` 使用 `hidden(true)`，与 **默认 `rg`**（不搜隐藏）不一致；与 `tools/prompts/file.md` 中「skip hidden」表述也可能冲突。  
- **拟定**：改为 **`walk.hidden(include_hidden)`**，其中 `include_hidden` 来自 JSON **`includeHidden`**，默认 **`false`**。  
- **风险**：依赖「搜点目录 / `.env`」的既有调用会 **少结果**；缓解：`file.md` 与 `COMMUNICATION` 中明确写出「默认不搜隐藏；需要时传 `includeHidden: true`」。  
- **版本**：建议在变更日志中标注 **行为变更**（若已有对外承诺「会搜隐藏」，需产品确认）。

---

## 4. `fileTypes` 预置表（建议）

用 **小表** 映射到 glob 列表，避免维护完整 `rg --type-list`：

| `fileTypes` 取值 | 展开为 `includeGlobs`（示例） |
|------------------|-------------------------------|
| `rust` | `["**/*.rs"]` |
| `typescript` / `ts` | `["**/*.ts", "**/*.tsx", "**/*.mts", "**/*.cts"]` |
| `javascript` / `js` | `["**/*.js", "**/*.jsx", "**/*.mjs", "**/*.cjs"]` |
| `vue` | `["**/*.vue"]` |
| `json` | `["**/*.json", "**/*.jsonc"]` |
| `markdown` / `md` | `["**/*.md", "**/*.mdx"]` |
| `toml` | `["**/*.toml"]` |
| `yaml` | `["**/*.yml", "**/*.yaml"]` |

未知 type：**返回明确错误**（列近邻建议），避免静默空结果。

**与 `includeGlobs` 关系**：`fileTypes` 只负责 **展开**；最终仍走同一套 glob 过滤逻辑（复用或对齐 `file:glob` 的 `GlobSet` 构建方式，注意 **Windows 路径分隔符** 归一）。

---

## 5. 实现要点（Rust）

### 5.1 Matcher 构建

```text
RegexMatcherBuilder::new()
  .fixed_strings(fixed_string)
  .case_insensitive(ignore_case)   // 或与 case_smart 二选一，建议先只做显式 ignoreCase
  .multi_line(false)
  .build(pattern)
```

- **`fixedString: true`** 时：禁止与「明显依赖正则」的文档示例混用；可选在 `fixedString` 为 true 时 **收紧 pattern 长度**（如仍 512）或略放宽（字面量可能较长）。  
- **`ignoreCase` + `fixedString`**：同时启用即可。

### 5.2 Walk + Glob 过滤

1. **阶段 A**：`WalkBuilder` 产出候选文件路径（已有 `git_ignore`、hidden、max_depth、扩展名二进制跳过）。  
2. **阶段 B**：若 `includeGlobs` / `fileTypes` 非空，对每条候选路径算 **相对 walk 根** 的 POSIX 风格相对路径，匹配 `GlobSet`（OR）；再应用 `excludeGlobs`。  
3. **注意**：glob 过滤在 **walk 之后** 做简单实现会浪费 IO；更优是 `walk.filter_entry` 内对 **目录** 做粗剪枝（可选优化项），MVP 可先做「walk 全量路径 + glob 过滤 + 大小过滤」再 grep（与当前大目录成本同阶，但 glob 会显著减少 `grep` 调用次数）。

### 5.3 并行

- **做法**：先收集本轮 **待 grep 文件路径** 到 `Vec<PathBuf>`（上限：例如最多 N 万个文件，超过则 `warn` 并截断或要求缩小 `path` / glob）。  
- **执行**：`matcher` + `Searcher` 配置 **每线程克隆** 或 **每文件新建 Searcher**（`SearcherBuilder::build()` 较轻）；使用 `rayon::par_iter` 对路径并行，结果写入 **`Mutex<Vec<...>>`** 或 **分片 vec 再 merge**，最后按 `(path, line)` 排序（可选，若需稳定输出）。  
- **停止条件**：全局 `maxResults` 达到后，需 **尽早取消** 剩余任务：`AtomicBool` + 在每文件开始时检查，或接受「多搜几条再截断」的 MVP（实现更简单）。  
- **依赖**：`Cargo.toml` 增加 `rayon`（若尚未引入）。

### 5.4 单文件上限与 `maxResults`

| 项 | 现状（代码常量） | 建议 |
|----|------------------|------|
| 单文件跳过阈值 | `MAX_GREP_FILE_BYTES = 2 MiB` | 升为 **8 MiB** 或 **可配置**（硬顶如 32 MiB），并在 JSON 增加可选 **`maxFileBytes`**（clamp），便于大文件日志类场景。 |
| 全局命中 | `MAX_GREP_RESULTS = 200` | 保持或略升（如 300），仍以 **响应体预算** 为主；与 `contextLines` 联动估算每 hit 体积。 |

---

## 6. 文档与提示词

- 更新 **`crates/pointer-core/src/tools/prompts/file.md`**：`file:grep` 参数表、默认 hidden、示例（`includeGlobs` + `fileTypes` + `fixedString`）。  
- **不写**开发文件名进提示词正文以外的 agent 文档时，遵守仓库既有规范。  
- 在 **[`coder-agent-capability-roadmap.md`](coder-agent-capability-roadmap.md)** §5.4 链到本文档作为 **D 的落地方案**。

---

## 7. 测试计划

| 用例 | 期望 |
|------|------|
| 默认 `includeHidden` 缺省 | `.hidden` 目录下文件 **不被** 搜到；`includeHidden: true` 搜到。 |
| `fixedString: true` | `pattern` 为 `a.b` 只匹配字面 `a.b`，不匹配 `acb`。 |
| `ignoreCase: true` | `foo` 匹配 `Foo`。 |
| `includeGlobs: ["**/*.rs"]` | 非 `.rs` 无命中。 |
| `fileTypes: ["rust"]` | 与 `**/*.rs` 一致。 |
| `excludeGlobs` | 排除 `**/target/**` 等（可与默认 exclude 表叠加，需定义优先级）。 |
| 并行 | 多文件临时目录，结果集与顺序实现一致（或只断言 multiset 相等）。 |

---

## 8. 分阶段交付（建议）

| 阶段 | 内容 | 粗估 |
|------|------|------|
| **Phase 1** | `fixedString`、`ignoreCase`、hidden 默认修正 + 文档 | **1–2 人天** |
| **Phase 2** | `includeGlobs` / `excludeGlobs` / `fileTypes` + 单测 | **2–4 人天** |
| **Phase 3** | 并行 +（可选）`maxFileBytes` 与单文件上限上调 + 跳过统计 | **2–4 人天** |

合计约 **M** 档（5–10 人天）量级，视并行取消策略与 UI/日志 要求浮动。

---

## 9. 风险与缓解

| 风险 | 缓解 |
|------|------|
| Hidden 默认变更导致「搜不到」 | 发布说明 + 提示词显式列举何时需要 `includeHidden` |
| 并行 + 大 vec 内存峰值 | 限制 walk 产出文件数、保持单文件上限、流式 walk（后续优化） |
| Glob 与绝对路径根不一致 | 统一「相对 walk 根」的字符串与 `file:glob` 对齐并加集成测试 |

---

## 10. 与「封装 `rg`」路线的关系

- **本方案**：零外部二进制、跨平台一致、可精细控制 JSON 与限额。  
- **封装 `rg`**：CLI 行为与本地开发者习惯 1:1，但引入路径发现、版本与安全审查。  
- **建议**：先落地本方案 **Phase 1–2**；若仍有大仓性能瓶颈，再评估 **并行 + mmap 级优化** 或 **可选 rg 后端**。
