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

- **给模型的 md**（`prompts/`、`tools/prompts/`）：主提示词写场景和规则，工具参数写在对应工具文档里。不要写填空模板（`Next` / `Repetition` / `Locate` / 比例公式）。**不要**写文件名、合并方式、manifest、档位标签、跨档引用等开发信息。
- **中途 `content`：** 与 `COMMUNICATION_PUBLIC` 一致——有桌面工具时默认空；过程进度走工具行和任务板。有工具时仅用户必须行动或必须知情的决策可写。澄清/完成回合仍必须非空。
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
| **user `[CUR_SCREEN]`** | 槽位标签 + 3–4 图（可选上一帧复用、当前屏、标注屏、指针放大图）+ 操作历史 + runtime + **Pointer position** + **Nearby overlay reference bboxes**（指针最近 **10** 条）。上一帧用已保存的原图，不再截一次 |

**Verify**：宿主在 desktop tool 执行后自动校验（`computerPipelineVerifyHost`）；模型读 history 行 `verify:` 后缀，**不再**调用 `action_verify` sidecar。

详见 [`../internals/llm-prompt-assembly-order.md`](../internals/llm-prompt-assembly-order.md)、[`computer-verify-host.md`](computer-verify-host.md)。

## 三档差异（摘要）

| 档位 | 图像 | 提示词 | 模型 / 思考 |
|------|------|--------|-------------|
| Primary | 3–4 图：可选复用的上一帧 + after + annotated + pointer zoom | Primary | `computerModelPrimary`，预算 2048 |
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

主提示词只写场景，不写填空模板。工具参数在各工具自己的 `tools/prompts/*.md`。

按这个顺序选一条：

1. **序号** — `[Annotated after action]` 上的序号标的就是该控件时，用 `*_index`。整行、整张卡片或整块面板上的序号标的是区域，不是里面的小控件。序号每屏重置。
2. **大对象坐标** — 全屏上能看见可点的内部，点落在内部仍然在控件上：窗口、面板、输入框、列表行、大按钮、菜单项。没有可用序号时，在 `[Screen after action]` 上用 `*_at`，不带 `zoom`。
3. **小对象** — 只是一个记号，全屏上看不到能瞄准的内部：图标、复选框、光标、字形，或贴在大区域边缘的控件。本轮只把指针移到旁边，不点击。旁边指：指针和目标在同一可见区域（同一行、同一组按钮或同一条边缘），空隙短到下一轮放大图能包住目标。邻控件已有序号就移到那个序号；目标贴在大区域边缘时，落在该区域里靠目标一侧的空白，用偏向这一侧的粗坐标。区域序号会点到中心，这一拍不用。下一轮目标在 `[Zoom pointer after action]` 里时，坐标工具带 `zoom: true`，`x`/`y` 是放大图自己的 0–1000（左上 0，右下 1000）。宿主按本轮裁切窗口映射回全屏坐标。序号和大对象坐标不要带这个标记。目标不在放大图里就再移一次，这一拍不点击。

`[Screen before action]` 是上一轮已经截下的原图，下一轮直接复用，不再为它重新截屏。规划不拿它覆盖宿主 `verify:`。Verify 另有自己的操作前/操作后。

工具正文：`tools/prompts/mouse.md`、`input.md`、`modified_click.md` 等。组装：`generate_tools_system_appendix`。

应用列表/启动：`tools/prompts/list_apps.md`、`launch_app.md`；跨平台实现见 [`computer-app-access.md`](computer-app-access.md)。

## 外部 Agent 目录覆盖

自定义 `computer` 目录时，communication 优先读：

1. `prompts/tiers/primary/communication.md`
2. 根目录 `COMMUNICATION.md`（兜底）
