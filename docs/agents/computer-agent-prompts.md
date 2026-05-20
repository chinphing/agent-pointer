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
| **Location:** | overlay 路由 + 可选 (x,y) | Screen after → 选一个 overlay frame → L4 用 Annotated |
| **Tool route:** | 选工具 | 不再读图，查表执行 |

---

## Location 路由表（简）

| 路径 | 条件 | Line 3 | Line 4 |
|------|------|--------|--------|
| L0 | 非 overlay | n/a | — |
| L1 | 有 bbox，`wrap count=1` | index path | 无 |
| L2 | 有 bbox，`wrap count>1` | coordinate path | anchor 在 reference bboxes → (x,y)，否则 deferred |
| L3 | 无 bbox+label（unmarked） | coordinate path | 以相邻 index R 为锚，同上 |

**Marked vs unmarked 判定（§5 T1–T3）：** 先看 **digit flush 在哪条 bbox 边框上** — 该 bbox 才是 index 所指区域。**T3 关键：** 该 bbox 是否**几何包含** intended sub-target 的可点击区域？

- **包含** → marked → `therefore selected overlay index N`
- **不包含**（113 在聊天列表/侧栏等邻居框上，输入框在框外）→ unmarked → `therefore adjacent reference index R=113`；**(c) 必须写包裹邻居区域，不能写包裹输入框**

例：消息输入框无自有 digit；113 flush 于上方/左侧聊天列表 bbox → **unmarked，R=113**（~~selected index 113~~ ~~(c)包裹输入框~~ 为常见误判）。

---

## inject 文案约定

- **`reference_anchors.rs`** / **`screen_inject.rs`** — 只描述**注入数据格式**（Pointer position、reference bboxes 字段含义）。
- **分析逻辑与路由** — 只在 **`COMMUNICATION.md`**，不在 inject 重复 if-else 规则。

---

## 维护 checklist（改分支时）

1. 更新 **路由表**（唯一分支源）
2. 更新对应 **步骤表** 与 **模板**（marked / unmarked 分离）
3. 更新 **1 个 golden example**
4. 在 **COMMUNICATION_FULL.md** 补 anti-pattern
5. **禁止**在 inject 文案里加分析逻辑
