# Computer 子 Agent 提示词结构

内置 **computer** Agent 的系统提示由三部分拼接而成（磁盘覆盖 Agent 目录时行为一致）：

1. **`COMMUNICATION_SHARED.md`** — 图像槽位 **Frame registry**（每个 slot 哪个 stage 必须读）。
2. **`COMMUNICATION.md`**（**运行时 slim**）— **Proof discipline** + **Image discipline** + 六阶段 **步骤表** + **路由表** + 每阶段 1 个 golden example。
3. **平台相关提示词**（`OS_MACOS.md` / `OS_WINDOWS.md` / `OS_LINUX.md`）。
4. **`AGENT.md` 正文** — 角色与循环要点。

扩展样例与 anti-pattern：**`COMMUNICATION_FULL.md`**（**不**随运行时加载）。

---

## 两大全局原则（运行时 COMMUNICATION.md §Global discipline）

### A) 证明式推导

- 六阶段 **严格按序**；每阶段内 **编号行按序**，**禁止跳步**。
- **先分析，再结论** — 含判断的每一行：**Analysis 在前**，**`Conclusion:` / `therefore` 在最后**；禁止行首写结论再补分析。

### B) 图像依据（禁止文本瞎猜）

- 任何像素/布局/控件/指针/overlay 描述必须以 **`On [Frame name]:`** 开头，引用当前 **`[CUR_SCREEN]`** 中的 slot。
- **Stage 1–4** 禁止 overlay 编号；**Location line 1** 禁止 overlay 编号。
- 各 stage 读哪张图见 **Frame registry**（COMMUNICATION_SHARED + COMMUNICATION §C）。

---

## 六阶段分工

| Stage | 决定什么 | 主要读图 |
|-------|---------|---------|
| **Pointer:** | 上一动作指针热点 vs 目标中心 | `[Zoom pointer before action]` |
| **Verify:** | 上一动作是否成功 | `[Screen before action]` → `[Screen after action]` |
| **Repetition:** | 是否 stuck | `[Recent desktop tool calls]` |
| **Next:** | 本轮做什么 | `[Screen after action]` line 2 |
| **Location:** | **reference index R** + **(x,y)** | Screen after → 选一个 overlay frame → L3 用 Annotated + inject |
| **Tool route:** | 选工具 | 不再读图 — 一律 **`*_at(x,y)`** |

---

## Location（全坐标实验 — 简）

**原则：** overlay **index 仅作 Location 锚点**；**全回合禁用 `*_index`**，一律 **`*_at(x,y)`** 坐标方法。

**禁止：** 任何 **`click_index`** / **`type_text_at_index`** / **`modified_click_index`** 及 **`tool_args` 中的 index 字段**。

| 行 | 内容 |
|----|------|
| L1 | **Placement→frame** — `[Screen after action]` 方位 → 选一个 overlay frame |
| L2 | **Reference index R** — intended sub-target；bbox **R** 内容；**`distinct hit targets = N`** |

**Anchor gate：** **N > 1** → L3 **禁止** bbox center，必须 **corner + offset** 到 intended sub-target。

**禁止：** Location 已有 **`therefore (x,y)`** 却用 **`click_index`** — 本会话 **所有回合** 均禁止 **`*_index`**，必须 **`click_at`** + 相同 **x/y**。
| L3 | **Coordinate geometry** — **I1** 布局 → **I2** 从 inject row **R** **抄写 (xa,ya) 字面量** → **I3** offset → **I4** 算术 → **`therefore (x,y)`** |

**禁止：** 只写 anchor 名称或最终 (X,Y)，不先 quote inject 里的数字。

**选 R / anchor 类型：** 见 **COMMUNICATION.md** §5 — multi-control 用 corner + offset；单控件可用 center。

**非 overlay：** **`Location: n/a`**

---

## inject 文案约定

- **`reference_anchors.rs`** / **`screen_inject.rs`** — 注入 **Pointer position** + **Overlay reference bboxes**（**全部** index 的 corner/center，按 index 排序）。
- **分析逻辑与路由** — 只在 **`COMMUNICATION.md`**，不在 inject 重复 if-else 规则。

---

## Tool route（坐标 triple-lock）

Location L3、Tool route recap、line **2**、root **`tool_args`** 四处 **`x`/`y` 字面量必须相同**。

**禁止：** `mouse:click_at at computed (x,y)` — line **2** 必须写 `goal; action; x: …; y: …`。

**小图标 / 多控件 bbox 内 sub-target：** corner + offset（禁止 row center 当点击点）。

**Inject lookup（L3 必写）：** `inject row R <anchor>: (xa, ya) = (…, …)` → offset → arithmetic → `(X, Y)`。

## 维护 checklist（改分支时）

1. 更新 **Location 三行模板** 与 **Tool route 执行表**（坐标唯一路径）
2. 更新 **1 个 golden example**（含 reference index + click_at）
3. 在 **COMMUNICATION_FULL.md** 补 anti-pattern
4. **禁止**在 inject 文案里加分析逻辑
5. 确认 **`reference_anchors` 测试**通过（全 index 注入、按 index 排序）
