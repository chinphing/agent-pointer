# Computer 子 Agent 提示词结构

Computer Agent 使用 **三级运行时档位**（Primary / Intermediate / Advanced），每档有独立的 communication、循环正文、图像注入与模型设置。

## 目录结构（`crates/pointer-core/src/agents/computer/`）

```
computer/
  AGENT.md                 # manifest（id / config / tools），正文仅简短角色说明
  mod.rs                   # 模块入口 + 对外 re-export
  state/ input/ vision/ tier/   # Rust 实现（见 `computer/README.md`）
  verify.rs capture_debug.rs
  extension_hooks/ tools/
  prompts/
    tiers/
      primary/             communication.md + loop.md
      intermediate/        communication.md + loop.md
      advanced/            vision_slots.md + communication.md + loop.md
    os/                    macos.md | windows.md | linux.md
    README.md              # 维护索引（不加载）
  author/                  # 不参与运行时（样例 / 长文参考）
    communication_full.md
    agent_body_full.md
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
| Primary | `tiers/primary/communication.md` | `tiers/primary/loop.md` |
| Intermediate | `tiers/intermediate/communication.md` | `tiers/intermediate/loop.md` |
| Advanced | `tiers/advanced/vision_slots.md` + `communication.md` | `tiers/advanced/loop.md` |

OS 片段：`prompts/os/{macos,windows,linux}.md`，三档共用。

## System 组装与 Context Cache

| 分区 | 内容 |
|------|------|
| **cacheable（第 1 段）** | `COMMUNICATION_PUBLIC` + 当前档 communication + loop + tools + `[Environment]` + JSON wire |
| **dynamic（第 2 段）** | `[TASK_BOARD]`、`[LOCKED GOAL]`（有锁时） |
| **user `[CUR_SCREEN]`** | 槽位标签 + 图 + 操作历史 + runtime + **Pointer position** + bbox 坐标表（Primary/Intermediate：**Nearby overlay reference bboxes** 指针最近 **10** 条；Advanced：全量 **Overlay reference bboxes**） |

**Primary Verify**：对比 **Expected**（上一工具 **goal** 要求的界面变化）与 **Actual**（当前截图）；仅指针到位 ≠ **pass**（见 communication 反例）。**Next**：**MA-0…MA-9** 分支 **HOVER** / **PRECISION**，**Nearby** 须与注入 bullet 字符级一致。

升档时 cacheable 中的 communication 切片会替换，前缀缓存失效一次（可接受）。

详见 [`../internals/llm-prompt-assembly-order.md`](../internals/llm-prompt-assembly-order.md)、[`../llm/qwen-context-cache.md`](../llm/qwen-context-cache.md)。

## 三档差异（摘要）

| 档位 | 图像 | 内部推理 | 模型 / 思考 |
|------|------|----------|-------------|
| Primary | 2-3 图：可选 **`[Screen before action]`** + **`[Screen after action]`** + **`[Annotated after action]`**；无 zoom | 内部跑 Verify / Repetition / Next；**N–目标关系** 决定 index vs coordinate；**不写** assistant 正文（除最终回复）；每轮 **`verify:report`** | qwen3.5-plus |
| Intermediate | 原图 + marked + Annotated；**无** zoom/before | 内部 **Verify→Pointer（条件）→Repetition→Next**；**Parameter** 块 per index arg；**Nearby bboxes×10**；正文默认空；**`verify:report`** | qwen3.5-plus，思考预算 **2048** |
| Advanced | 7 槽（与现网一致） | 内部七段 **Verify→…→Tool route**；coordinate 为主，可选 index；正文默认空；**`verify:report`** | qwen3.6-plus，思考 8K |

## 操作历史

- 每档独立 `[Recent desktop tool calls]` 列表，最多 10 条。
- 每行必填 **`goal="…"`**，坐标为 session 0–1000 **(x,y)**（不写 index）。
- verify：Primary 历史行仅 `pass/fail/pending/n/a`（无 cause）；Intermediate/Advanced 可带 `(cause)`。

## Repetition 计数与升档

升档由宿主运行时根据 sidecar 信号执行，不向模型注入独立 tier runtime 块。

| 字段 | 含义 |
|------|------|
| **action_result** | sidecar `verify:report` 上报，写入历史行 `action_result: ...` |
| **repetition_count** | sidecar `verify:report` 上报，宿主按阈值内部判定是否升级 |

模型在内部 **Repetition** 阶段计算 **Count**，经 **`verify:report`** 的 `repetition_count` 上报；升档信号不从 assistant 正文提取；升档由宿主在回合结束后执行，下一回合自动使用更高档模型/图像/提示词。
上一轮 verify 结果通过历史行内的 `verify: ...` 字段注入（不再追加单独汇总行）。

## 配置（`AGENT.md` config）

- `computerAutoUpgrade` — 是否自动升档（默认 true）
- `computerInitialTier` — `primary` | `intermediate` | `advanced`（设置 → 执行智能体 → Computer → **初始级别**；覆盖 AGENT.md 默认值，仅影响**新会话**起始档）
- `computerModelPrimary` / `computerModelAdvanced` — 可选覆盖模型 id

## 定位方式（按档）

| 档位 | 偏好 | 工具注册名 |
|------|------|------------|
| Primary | **hybrid**（index + coordinate，按中心归属切换） | `mouse` / `composite_action` / `modified_click` |
| Intermediate | **index**（`click_index`、`type_text_at_index` 等） | 同上 |
| Advanced | **coordinate 为主**（`click_at` + Location + Overlay bboxes）；也允许 `*_index` | 同上 |

工具正文：`tools/prompts/mouse.md`、`composite_action.md`、`modified_click.md`（按 **method** 后缀路由，不再拆分 `mouse_index` 等独立工具 id）。组装：`generate_tools_system_appendix`。

## Advanced 七阶段

`prompts/tiers/advanced/communication.md`：证明式七阶段、坐标 `*_at`、reference index R 仅作锚点。Primary 为 hybrid（index + coordinate），Intermediate 维持 index-only。

## Sidecar-only 回合（宿主）

Computer 每轮可含 sidecar（`verify.report`、`task_board.*`）与至多一个 root 桌面工具。

| 工具批次 | `content` | 宿主行为 |
|----------|-----------|----------|
| 仅 sidecar | 非空 | 执行 sidecar 后 **结束本轮 run**（用户可见回复） |
| 仅 sidecar | 空 | 执行 sidecar 后注入 **【提示】** recovery，继续 loop |
| 含 root 桌面工具 | 任意 | 正常进入下一轮截图 |

实现：`agents/computer/sidecar_turn.rs`，挂接于 `chat_service/single_agent.rs`。

## 外部 Agent 目录覆盖

自定义 `computer` 目录时，Advanced communication 优先读：

1. `prompts/tiers/advanced/vision_slots.md` + `communication.md`
2. 兼容旧路径：`shared.md`、`COMMUNICATION_SHARED.md`、根目录 `COMMUNICATION_ADVANCED.md`
