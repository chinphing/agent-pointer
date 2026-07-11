# Computer 子 Agent 提示词结构

Computer Agent 使用 **三级运行时档位**（Primary / Intermediate / Advanced）做模型升级；**提示词与截图布局三档相同**（Primary `communication.md` + `loop.md`），仅 LLM 模型与 thinking budget 按档切换。

## 目录结构（`crates/pointer-core/src/agents/computer/`）

```
computer/
  AGENT.md                 # manifest（id / config / tools），正文仅简短角色说明
  mod.rs                   # 模块入口 + 对外 re-export
  state/ input/ vision/ tier/   # Rust 实现（见 `computer/README.md`）
  capture_debug.rs
  extension_hooks/ tools/
  prompts/
    tiers/
      primary/             communication.md + loop.md（全 tier 运行时来源）
    modules/verify/        host verify LLM 提示词（按 OperationFamily）
    os/                    macos.md | windows.md | linux.md
    README.md              # 维护索引（不加载）
  author/                  # 不参与运行时（样例 / 长文参考）
  tools/                   # 工具 handler + schemas + prompts/*.md
  extension_hooks/
  assets/
```

## 提示词编写约定

- **给模型的 md**（`prompts/`、`tools/prompts/`）：只写合并后模型能直接执行的规则与示例；**不要**写文件名、合并方式、manifest、档位标签、跨档引用等开发信息。
- **开发说明**：本页、`prompts/README.md`、`author/` 下的文件。

## 运行时合并

`computer_communication_for_tier(tier)` + `computer_agent_body_for_tier(tier)`，段间 `\n\n---\n\n`。

| 档位 | Communication | Loop |
|------|---------------|------|
| Primary / Intermediate / Advanced | `tiers/primary/communication.md` | `tiers/primary/loop.md` |

`tier` 参数保留用于 API 兼容；实现上忽略档位，恒返回 Primary 切片。

OS 片段：`prompts/os/{macos,windows,linux}.md`，三档共用。

共享 UI 片段：`prompts/ui_disabled_controls.md`（各类灰色/不可操作控件类型、常见原因与 agent 规则），三档共用。

## System 组装与 Context Cache

| 分区 | 内容 |
|------|------|
| **cacheable（第 1 段）** | `COMMUNICATION_PUBLIC` + Primary communication + loop + tools + `[Environment]` |
| **dynamic（第 2 段）** | `[TASK_BOARD]`、`[LOCKED GOAL]`（有锁时） |
| **user `[CUR_SCREEN]`** | 槽位标签 + 2–3 图 + 操作历史 + runtime + **Pointer position** + **Nearby overlay reference bboxes**（指针最近 **10** 条） |

**Verify**：宿主在 desktop tool 执行后自动校验（`computerPipelineVerifyHost`）；模型读 history 行 `verify:` 后缀，**不再**调用 `action_verify` sidecar。

详见 [`../internals/llm-prompt-assembly-order.md`](../internals/llm-prompt-assembly-order.md)、[`computer-verify-host.md`](computer-verify-host.md)。

## 三档差异（摘要）

| 档位 | 图像 | 提示词 | 模型 / 思考 |
|------|------|--------|-------------|
| Primary | 2–3 图：可选 before + after + annotated | Primary | `computerModelPrimary`，预算 2048 |
| Intermediate | 同上 | Primary | `computerModelIntermediate`，预算 2048 |
| Advanced | 同上 | Primary | `computerModelAdvanced`，预算 8192 |

## 操作历史

- 每档独立 `[Recent desktop tool calls]` 列表，最多 10 条。
- 每行末尾由宿主维护 **`verify:`** 后缀（`verifying` / `verified - pass|fail|…` / `skipped`）。
- 升档由宿主根据 verify fail 阈值执行；**pass** 重置到 `initial_tier`。

## 配置（`AGENT.md` config）

- `computerAutoUpgrade` — 是否自动升档（默认 true）
- `computerInitialTier` — `primary` | `intermediate` | `advanced`
- `computerModelPrimary` / `computerModelIntermediate` / `computerModelAdvanced`
- `computerPipelineVerifyHost` — 宿主 post-execute verify（默认 true）
- `computerPipelineModelVerify` / `computerPipelineThinkingBudgetVerify` — verify LLM

## 定位方式

三档均支持 **index + coordinate**（N–目标关系决定 `*_index` vs `*_at`）。

工具正文：`tools/prompts/mouse.md`、`input.md`、`modified_click.md` 等。组装：`generate_tools_system_appendix`。

应用列表/启动：`tools/prompts/list_apps.md`、`launch_app.md`；跨平台实现见 [`computer-app-access.md`](computer-app-access.md)。

## 外部 Agent 目录覆盖

自定义 `computer` 目录时，communication 优先读：

1. `prompts/tiers/primary/communication.md`
2. 根目录 `COMMUNICATION.md`（兜底）
