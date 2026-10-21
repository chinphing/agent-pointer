# 电脑操控层重构设计

> **状态：P1–P5 已实施，P0 验证完成。** 决策见文末 §10，实测数据见附录。

## 1. 背景

Pointer 现有的电脑操控链路依赖一个**外部标注服务**（`POST {annotateApiBase}/api/v1/annotate/all`）：
截图上传 → 服务返回带编号的标注图 + 元素框 → 主 LLM 看图后调 `mouse_click_index`（按编号）
或 `mouse_click_at`（按 0–1000 坐标）→ 点击 → 宿主 Verify LLM 判 pass/fail。

同时，独立原型 **B5**（`benchmarks/screenspot_grid/baselines/b5_decoupled.py`）在
ScreenSpot-Pro ab100 上达到 **79.0%**（均值 31.4s），走的是另一条路：
视觉模型直接输出 bbox → 以 bbox 为中心裁剪放大并画蓝框 → 视觉模型判"是不是目标" → 重定位。

本次重构要解决三件事：

1. 删除外部标注服务，去掉 `_index` 一族工具；
2. 动作语义对齐 OSWorld 的 `computer_13`；
3. 把电脑操控抽象成接口，本地实现与 OSWorld 远程评测实现共用一套语义。

---

## 2. 目标与非目标

### 目标

| 项 | 内容 |
|---|---|
| 动作语义 | 严格对齐 OSWorld `computer_13`（13 个动作 + 3 个终止信号） |
| 抽象 | `ComputerControl` trait，`LocalControl` / `RemoteControl` 两个实现 |
| 删除 | 外部标注服务、全部 `_index` 工具、`reference_anchors` |
| 保留 | 所有非 13 的既有能力（剪贴板、应用管理、元素绑定输入、区间选择等） |
| 接入 | B5 定位逻辑 |

### 非目标

- 不在本次验证 B5 在真实产品界面上的效果（见 P0，必须先做）
- 不改变主 LLM 的规划层
- 不改动 `benchmarks/` 下的评测代码

---

## 3. 总体架构

```
┌──────────────────────────────────────────────────────┐
│ L3  规划      主 LLM：决定做什么                       │
└──────────────────────┬───────────────────────────────┘
                       │ 文字描述
┌──────────────────────▼───────────────────────────────┐
│ L2  定位      ElementLocator（B5）                     │
│              截图 → 视觉模型出框 → 裁剪放大画蓝框        │
│              → 校验 → 重定位（≤3 轮）→ 坐标             │
└──────────────────────┬───────────────────────────────┘
                       │ (x, y)
┌──────────────────────▼───────────────────────────────┐
│ L1  执行      ComputerControl（13 动作）                │
│              ├ LocalControl  (enigo + xcap)           │
│              └ RemoteControl (HTTP → OSWorld VM)      │
└──────────────────────┬───────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────┐
│ L0  观测      screenshot() / screen_size()             │
└──────────────────────────────────────────────────────┘
```

**关键判断：13 个动作全部是「怎么动」，没有一个是「往哪动」。**
B5 属于 L2 定位层，**不是第 14 个动作**，不应塞进 `ComputerControl`。

---

## 4. L1 执行层

### 4.1 动作集（对齐 OSWorld）

OSWorld 权威定义见 `desktop_env/controllers/python.py::execute_action`。

| # | 动作 | 参数 | 编译成的 pyautogui |
|---|---|---|---|
| 1 | `MOVE_TO` | x, y | `moveTo(x, y, duration, ease*)` |
| 2 | `CLICK` | button, x, y, num_clicks | `click(button=, x=, y=, clicks=)` |
| 3 | `MOUSE_DOWN` | button | `mouseDown(button=)` |
| 4 | `MOUSE_UP` | button | `mouseUp(button=)` |
| 5 | `RIGHT_CLICK` | x, y | `rightClick(x=, y=)` |
| 6 | `DOUBLE_CLICK` | x, y | `doubleClick(x=, y=)` |
| 7 | `DRAG_TO` | x, y | `dragTo(x, y, duration=1.0, button='left', mouseDownUp=True)` |
| 8 | `SCROLL` | dx, dy | `hscroll(dx)` + `vscroll(dy)` |
| 9 | `TYPING` | text | `typewrite(repr(text))` |
| 10 | `PRESS` | key | `press('key')` |
| 11 | `KEY_DOWN` | key | `keyDown('key')` |
| 12 | `KEY_UP` | key | `keyUp('key')` |
| 13 | `HOTKEY` | keys[] | `hotkey('a', 'b')` |

终止信号：`WAIT` / `FAIL` / `DONE`（在 `execute_action` 内均为 no-op）。

> **13 个动作 + 3 个终止信号。** 终止信号不是动作，不计入动作集。
> 曾怀疑存在第 17 个动作，核对 OSWorld 源码后确认没有（见 §10 第 1 条）。

### 4.2 trait 定义

```rust
pub trait ComputerControl: Send + Sync {
    // ── 13 个动作 ──
    fn move_to(&self, x: i32, y: i32) -> Result<()>;
    fn click(&self, button: MouseButton, x: Option<i32>, y: Option<i32>, clicks: u32) -> Result<()>;
    fn mouse_down(&self, button: MouseButton) -> Result<()>;
    fn mouse_up(&self, button: MouseButton) -> Result<()>;
    fn right_click(&self, x: i32, y: i32) -> Result<()>;
    fn double_click(&self, x: i32, y: i32) -> Result<()>;
    fn drag_to(&self, x: i32, y: i32) -> Result<()>;
    fn scroll(&self, dx: i32, dy: i32) -> Result<()>;
    fn typing(&self, text: &str) -> Result<()>;
    fn press(&self, key: &str) -> Result<()>;
    fn key_down(&self, key: &str) -> Result<()>;
    fn key_up(&self, key: &str) -> Result<()>;
    fn hotkey(&self, keys: &[String]) -> Result<()>;

    // ── 终止信号 ──
    fn wait(&self, ms: u64) -> Result<()>;

    // ── 观测 ──
    fn screenshot(&self) -> Result<Vec<u8>>;
    fn screen_size(&self) -> Result<(u32, u32)>;
    fn cursor_position(&self) -> Result<(i32, i32)>;   // 仅诊断，非必需
}
```

### 4.3 `MouseButton` 必须扩到三值

**现状**（`crates/pointer-core/src/agents/computer/input/actions.rs:27`）：

```rust
pub enum MouseButton {
    Left,
    Right,
}
```

**改为：**

```rust
pub enum MouseButton { Left, Middle, Right }
```

- enigo 0.2.1 原生支持 `enigo::Button::Middle`，**不需要换依赖**；
- `input/enigo.rs:241` 的 `match` 是穷尽匹配，加变体后编译器会指出全部改动点（当前仅 1 处）。

### 4.4 观测契约（必须写入 trait 文档）

> **`screenshot()` 与 `screen_size()` 必须处于同一坐标系。**

| 实现 | 截图 | `screen_size` | 是否一致 |
|---|---|---|---|
| Pointer 本地 | `xcap` 抓物理像素 → 重采样到**逻辑**分辨率 | 逻辑像素 | 是 |
| OSWorld 远程 | VM 1920×1080 | 1920×1080 | 是 |

目前两边一致是巧合，不是契约。若不一致，L2 的 0–1000 → 像素换算会系统性偏移。

---

## 5. 两个实现

| 实现 | 底层 | 用途 |
|---|---|---|
| `LocalControl` | `enigo`（输入）+ `xcap`（截屏） | 本机操控 |
| `RemoteControl` | HTTP → OSWorld VM `/execute`、`/screenshot`、`/screen_size` | 评测 |

`RemoteControl` 把每个方法编译成一句 pyautogui 字符串，与 OSWorld 源码逐字对齐：

| trait 方法 | 生成的代码 |
|---|---|
| `move_to(x,y)` | `pyautogui.moveTo(x, y, dur, easeInQuad)` |
| `click(Left,Some(x),Some(y),1)` | `pyautogui.click(button='left', x=x, y=y)` |
| `click(Left,Some(x),Some(y),2)` | `pyautogui.click(button='left', x=x, y=y, clicks=2)` |
| `mouse_down(Middle)` | `pyautogui.mouseDown(button='middle')` |
| `mouse_up(Middle)` | `pyautogui.mouseUp(button='middle')` |
| `right_click(x,y)` | `pyautogui.rightClick(x=x, y=y)` |
| `double_click(x,y)` | `pyautogui.doubleClick(x=x, y=y)` |
| `drag_to(x,y)` | `pyautogui.dragTo(x, y, duration=1.0, button='left', mouseDownUp=True)` |
| `scroll(dx,dy)` | `pyautogui.hscroll(dx)` + `pyautogui.vscroll(dy)` |
| `typing(t)` | `pyautogui.typewrite(repr(t))` |
| `press(k)` | `pyautogui.press('k')` |
| `key_down(k)` / `key_up(k)` | `pyautogui.keyDown('k')` / `pyautogui.keyUp('k')` |
| `hotkey([a,b])` | `pyautogui.hotkey('a', 'b')` |

**字符串生成是纯函数，可以脱离 VM 单测**——这是验证两个实现语义一致的手段。

---

## 6. L2 定位层：B5 接入

### 6.1 B5 逻辑

```
① box = locate(整屏截图, 描述)              ← 视觉模型，输出 0-1000 归一化 bbox
② 循环（≤3 轮）
     side  = clamp(框长边 × 3, 240, 1600)
     crop  = 以 box 中心裁 side×side → 放大到 1600 → 画蓝框
     v     = verify(crop, 描述)
     ├ yes                → 完成
     ├ unclear + 锚点不可见 → side×2，重试（≤2 次）
     ├ unclear + 目标太小   → side÷2，重试（≤2 次）
     ├ unclear + 模糊       → 放弃，低置信
     └ no                 → 拼提示（"你上次框里是 X" + 方向）→ 重定位
                            有方向 → 在裁剪图内重定位
                            无方向 → 回整屏
                            几乎没动（每维 |Δ|≤2）→ 强制升级到整屏
③ 输出 box 中心
```

### 6.2 LLM 与定位层的工具契约

**暴露给 LLM 的定位工具是独立的 `locate_element`，不把定位与点击合并。**
它支持一次请求定位多个目标；定位成功只返回候选结果，不执行任何输入动作。

概念接口：

```json
{
  "elements": [
    {"description": "保存按钮", "hint_box": [800, 35, 840, 55]},
    {"description": "地址栏"}
  ]
}
```

**参数形态已定：单个 `elements` 数组，每个元素自带 `description` 与可选的 `hint_box`。**
不使用平行数组，避免描述与 hint 长度错配。

**`hint_box` 是 4 个像素值 `[x1, y1, x2, y2]`，与基准 B5 `locate` 输出的 bbox 同形**
（见 §6.3.1）。不做「点 + 半径」这类变体；缺省即表示不提供 hint。

**批量上限：单次最多 3 个元素。** 超过 3 个直接返回参数错误，不做静默截断。

定位结果也按输入顺序逐项返回：

```json
{
  "results": [
    {"ok": true, "description": "保存按钮", "x": 820, "y": 45,
     "box": [800, 35, 840, 55], "confidence": "high",
     "what_i_saw": "蓝色软盘图标", "rounds": 2},
    {"ok": false, "description": "地址栏", "reason": "not_found",
     "what_i_saw": "当前截图中未能可靠确认目标"}
  ]
}
```

一个目标失败不应抹掉同批其他目标的结果。调用方只能使用 `ok=true` 项的坐标；失败项不能回退成模型猜测坐标直接点击。

### 6.2.1 坐标空间：LLM 面与 L1 工具用像素，B5 内部归一化

**两个坐标空间必须分开，不能混用。**

| 层 | 坐标空间 | 说明 |
|---|---|---|
| LLM 可见 | **屏幕像素（逻辑像素）** | `locate_element` 的 `hint_box` 入参与返回的 `box` / `x` / `y`，以及所有坐标类工具的 LLM 侧参数 |
| L1 工具 / 动作层 | **屏幕像素（逻辑像素）** | 切换 `CoordinateSystem::Pixel` 后 `resolve_coordinate` 透传，不再归一化 |
| B5 定位层 | **0–1000 归一化** | B5 全流程（`_norm2px`、`bounds_norm`、`rel_to_norm`）使用归一化坐标；转换由 `locate_element` 内部完成 |

**实现方案（已定）：全局切换到 `CoordinateSystem::Pixel`。**

现有坐标工具（`mouse_click_at`、`mouse_hover_at`、`mouse_drag_from_to_at`、`input_at`、
`modified_click_select_at`、`modified_click_range_select_at`）目前 LLM 侧收的是 **0–1000**，
来源是 `state/mod.rs:561` 把坐标系统写死成 `CoordinateSystem::Qwen`。
而 `vision/coord.rs` 里**已经有 `Pixel` 变体**，`normalized_to_screen` 直接透传。

因此**不需要逐个改工具的入参解析**：工具内部的 `model_point_to_session` / `resolve_coordinate`
本来就按 `coordinate_system` 分支，改公共层一处即全局生效。

**但 `Pixel` 分支当前有三个缺陷，必须先修，否则不能启用：**

| # | 缺陷 | 位置 | 修复 |
|---|---|---|---|
| 1 | **多显示器偏移丢失**：Qwen 分支加 `monitor.left/top`，Pixel 分支不加 | `coord.rs:52`、`coord.rs:76` | Pixel 分支同样做 `screen − monitor.left/top` 与反向加回 |
| 2 | **`screen_to_session_pair` 硬编码 Qwen** | `vision_state.rs:151` | 改为使用 `self.coordinate_system`；否则指针位置与点击坐标不同空间 |
| 3 | **zoom 语义在 Pixel 下不一致**：输入仍是「0–1000 of crop」，输出变像素 | `vision_state.rs:223` | 明确该路径的输入/输出单位并加注释与单测 |

**`locate_element` 与全局像素的衔接**：B5 内部全程是 0–1000，所以
`locate_element` 自己做一次 `像素 → 0–1000`（入参含 hint）与 `0–1000 → 像素`（返回值）。
这层转换与 `CoordinateSystem` 无关，由 `locate_element` 内部持有，不外泄。

**「像素」的定义**：指**逻辑像素**。截图已在 `screen.rs:251` 经
`resample_capture_to_logical` 重采样到逻辑分辨率，`MonitorInfo` 的 width/height 也是逻辑尺寸，
所以 Retina 屏上 LLM 看到的图与它估的像素值自洽。

**必做验收（单测，缺一不可）**

1. 多显示器：`MonitorInfo::new(1920, 0, 1920, 1080)` 下像素点往返误差 ≤ 1px（缺陷 1）。
2. 指针位置与点击坐标同一空间（缺陷 2）。
3. `locate_element` 返回的 `x/y` 原样传给 `mouse_click_at` 必须点中同一位置（端到端）。
4. 无 hint 与有 hint 两种调用下，返回坐标空间一致。

**同步要改的文案**：`tools/prompts/mouse.md`、`input.md`、`modified_click.md` 及其
`*.schema.yaml` 中「session 0–1000」的表述，改为像素并说明是逻辑像素。

### 6.3 后续坐标工具的调用约束

`locate_element` 只负责定位。`mouse_click_at`、`mouse_hover_at`、`mouse_drag_from_to_at`、`input_at` 等需要坐标的工具仍然独立暴露，但其工具说明和 computer-agent 提示词必须明确：

> 你对屏幕像素坐标的直接估计不可靠。操作界面中的可见目标前，先调用 `locate_element`；将它返回的 x/y 原样用于后续动作。不要自行从截图估坐标。只有用户明确给出坐标，或坐标来自刚才的 `locate_element` 结果时，才可直接调用坐标工具。

这样主 LLM 负责决定「要操作哪个目标」以及后续动作；`locate_element` 负责把描述转换成坐标；L1 工具执行坐标动作。定位和动作保持分离，可批量定位后串行执行动作。

`hint_box` 是主 LLM 给出的初步框，**不是最终点击坐标**。有它时跳过 B5 的整屏 locate，直接进入裁剪校验；没有它时从整屏 locate 开始。hint 不能绕过校验，也不能降低成功标准（见 §6.3.1）。

### 6.3.1 hint 的定义：预先给定的首轮 bbox

基准 B5 的 `locate` 工具输出 `bbox: [x1, y1, x2, y2]`，4 个 0–1000 整数
（`b5_decoupled.py:143-152`）；`_clamp_box` 负责钳制到 `[0,1000]` 并规范化为
`x1≤x2`、`y1≤y2`（`:55-57`）。

**因此本项目也把 hint 定成 bbox，而不是点。** 这样 hint 与 B5 首轮输出**同形**，
语义从「首轮定位的额外输入」变成「**预先给定的首轮框**」。

| | 点（原提案） | **bbox（已定）** |
|---|---|---|
| 形态 | `hint_x` / `hint_y` | `hint_box: [x1, y1, x2, y2]` |
| 与 B5 首轮输出 | 不同形，需另造一个框 | **完全同形** |
| 裁剪边长 | 需引入固定常数（如 480） | 沿用 B5 原规则 `clamp(长边×3, 240, 1600)` |
| 大小自适应 | 无 | 有：大按钮自动大裁剪，小图标自动小裁剪 |
| 畸形输入 | 无兜底 | 复用 `_clamp_box` 的钳制与重排序 |

**接入方式：有 `hint_box` 时跳过 B5 自己的整屏 locate，直接把 `hint_box` 当作首轮的框，
从第 2 轮（裁剪校验）开始跑。** 后续 verify → refine → retry 全部复用基准逻辑，不新增分支。

这样原先把 hint 当"首轮输入"时的三个风险被消除：

| 原风险 | 现状 |
|---|---|
| 提示词污染 | **消失**。根本不调 locate，没有新提示词 |
| 首轮几何被改掉 | **消失**。裁剪规则仍是 `clamp(长边×3, 240, 1600)`，只是框的来源变了 |
| 锚定效应 | **仍存在，但有防线**：校验器只看（裁剪图，描述），不看 `hint_box` |

**剩余风险只有一条：LLM 估的 bbox 是否比 B5 自己的整屏 locate 更差。** 这是可测的。

**必须保留的约束：**

- `hint_box` **绝不进入校验器**。校验器只看（裁剪图，描述）——否则它会把错框判成对的。
- `hint_box` 只允许**跳过 locate**，不允许**降低判定标准**；`confidence` 与无 hint 时一致。
- 无 `hint_box` 时行为与基准**完全一致**（整屏 locate 起步），退化路径可复现。
- **轮数预算不变**：locate 本来就在「≤3 轮」循环之外，跳过它不减少校验轮次。
- `hint_box` 做与基准相同的规范化：钳制到屏幕范围、重排为 `x1≤x2` / `y1≤y2`；
  退化框（面积为 0）按「未提供 hint」处理。

**验证方式（A/B 两组）：**

| 组 | 配置 | 判定 |
|---|---|---|
| A（基线） | 无 `hint_box`，跑完整 B5（首轮整屏 locate） | 基线 |
| B | 有 `hint_box`，跳过首轮 locate | 成功率 ≥ A |

**B 的确定收益**：省掉一次整屏 locate 的视觉模型调用（延迟下降），
这是独立于成功率的收益。若 B 的成功率显著低于 A，说明 LLM 的 bbox 不如 B5 自己看，
则退回 A（`hint_box` 只记日志，不传给定位层）。

**GUI 工具批次必须串行执行。** Pointer 已有 `ToolConflictClass::Computer` 与 batch planner 的串行机制；现有 `mouse_` / `input_` / `modified_click_` 前缀会被识别。新增的 `locate_element`、键盘动作等也必须显式归类为 `Computer`，不能依赖通用 fallback。这样同一轮里即使 LLM 并列发出多个 GUI 工具调用，宿主也会按顺序执行，避免并发截图、坐标过期和输入事件交错。当前实现位置：`crates/pointer-core/src/tools/parallel.rs`、`crates/pointer-core/src/chat_service/agent_tool_pass/batch.rs`、`crates/pointer-core/src/agents/computer/input/timing.rs`。

### 6.4 定位结果与动作结果

- `locate_element` 只确认「目标在当前截图中的位置」，不点击、不改变桌面状态。
- 点击、拖动、输入仍由 L1 对应工具执行；现有点击后 Verify 保留，负责确认动作是否生效。
- 点击前定位校验与点击后状态验证职责不同，不能互相替代。
- 定位失败必须返回结构化失败与观察摘要；不得硬点猜测坐标。
- 截图过期/屏幕变化校验暂不纳入本阶段设计；定位结果用于紧随其后的串行动作。

---

## 7. 工具面变更

### 7.1 暴露给 LLM 的工具清单

| 分组 | 工具 | 用途 |
|---|---|---|
| 定位 | `locate_element` | 把 `elements[]`（最多 3 项，每项 `description` + 可选 `hint_box`）转换为逐项结果；不执行 GUI 动作 |
| 坐标鼠标 | `mouse_hover_at` / `mouse_click_at` / `mouse_double_click_at` / `mouse_right_click_at` / `mouse_scroll_current` / `mouse_drag_from_to_at` / `mouse_down` / `mouse_up` / `mouse_drag_to` | 像素坐标或当前指针操作；涉及可见目标时先 `locate_element` |
| 键盘 | `input_at` / `input_focused` / `hotkey` / `key_press` / `key_down` / `key_up` / `keyboard_type` | 输入或按键；涉及屏幕目标时先定位，焦点操作仍须基于当前状态 |
| 既有系统能力 | `wait` / `clipboard_read` / `clipboard_write` / `list_apps` / `launch_app` | 等待、剪贴板与应用管理 |
| 既有复合操作 | `modified_click_select_at` / `modified_click_range_select_at` | 保留既有修饰键选择能力 |

**所有包含 GUI 操作的工具（包括 `locate_element`）均归类为 `Computer` 并串行执行。** 批量定位是单个工具内部对多个元素逐项处理，不是并发执行多个 GUI 工具调用。

**`click_element` 不暴露**：定位与点击分离，`click_element` 既不是 LLM 工具，也不是内部便利封装（见 §6.2）。

### 7.2 保留（非 13 的既有能力）

`clipboard_read` / `clipboard_write` / `list_apps` / `launch_app` /
`modified_click_select_at` / `modified_click_range_select_at` /
`input_at` / `mouse_drag_from_to_at`

### 7.3 改语义

| 工具 | 现状 | 改为 |
|---|---|---|
| **全部坐标类工具** | LLM 侧 0–1000 归一化 | **逻辑像素**（全局切 `CoordinateSystem::Pixel`，见 §6.2.1） |
| `mouse_click_at` | 无 `button` 参数 | 增加 `button`（left/middle/right，默认 left）与 `clicks` |
| `mouse_scroll_current` | 单位是行数 | 改为像素 `dx` / `dy` |
| `mouse_right_click_at` | 独立工具 | 保留为兼容别名，内部转 `click(Right, …)` |

### 7.4 新增

| 工具 | 对应动作 | 底层现状 |
|---|---|---|
| `mouse_down` | `MOUSE_DOWN` | 已有 `mouse_phase(button, Press)` |
| `mouse_up` | `MOUSE_UP` | 已有 `mouse_phase(button, Release)` |
| `key_press` | `PRESS` | 需新增 |
| `key_down` | `KEY_DOWN` | 需新增 |
| `key_up` | `KEY_UP` | 需新增 |
| `mouse_drag_to` | `DRAG_TO` | 需新增（从当前位置拖） |
| `keyboard_type` | `TYPING` | 需新增（当前焦点直接输入） |
| `locate_element` | L2 | 接入 B5；批量 `elements[]`，最多 3 项 |

### 7.5 删除

**工具（9 个 `_index` 变体）**

```
mouse_click_index           mouse_double_click_index
mouse_right_click_index     mouse_hover_index
mouse_scroll_index          mouse_drag_from_to_index
input_index                 modified_click_select_index
modified_click_range_select_index
```

**模块**

- `crates/pointer-core/src/agents/computer/vision/annotate.rs`（标注服务客户端）
- `crates/pointer-core/src/agents/computer/vision/reference_anchors.rs`（"指针附近 10 个 bbox"注入）
- `vision_state.rs` 的 `index_map` / `set_index_map_from_boxes` / `resolve_index`
- `AGENT.md` 中的 `annotateApiBase` 配置

---

## 8. 风险

### 8.1 删标注服务等于删掉 SoM，属能力降级

| | 现在 | 删掉后 |
|---|---|---|
| 「屏幕上有哪些可选元素」 | 标注服务列出全部 | 无 |
| 「这个元素在哪」 | 挑编号 | B5 定位 |

B5 只回答后者。且**从未比较过 B5 与 SoM 谁更好**——ScreenSpot-Pro 测的是
"给定描述找元素"，不是"从一堆标记里挑"。这是本方案最大的未验证假设。

### 8.2 `MOUSE_DOWN` 后进程崩溃会导致按键卡住

OSWorld 专门实现了 `release_stranded_input()`（Xlib 释放全部按键与 1–5 号鼠标按钮）。
本地实现需要同等兜底，否则后续每次点击都会变成"按住 Ctrl 点击"。

### 8.3 B5 尚无全量成绩

ab100 为 79.0%，M3 全量（1261 条）未跑。ab100 → M3 的换算浮动很大
（B4m +14.4pp、B3 +22.2pp），不能外推。

**P0 已在真实桌面上给出补充证据**（2026-10-21）：单张冻结桌面 20 个目标，
无 hint 20/20、近距错 hint 20/20、远距错 hint 20/20、不存在目标 5/5 诚实失败、
0 次自信错误。样本小（20 个目标、单一桌面），不能替代 M3 全量，
但至少证明 B5 在真实产品界面上不会"自信地框错"。详见附录。

---

## 9. 分期计划

| 阶段 | 内容 | 风险 | 验收标准 |
|---|---|---|---|
| **P0** | B5 在 Pointer 真实界面验证 | — | **已完成**：冻结桌面 20 个目标，无 hint 20/20、错 hint 20/20、不存在 5/5 诚实失败 |
| **P1** | `ComputerControl` trait + `LocalControl`（包装 enigo，行为不变） | 低 | 现有工具测试全绿，行为零变化 |
| **P2** | `RemoteControl` + pyautogui 字符串生成 | 低 | 每个方法生成的字符串与 OSWorld 源码一致（单测） |
| **P2.5** | 全局切 `CoordinateSystem::Pixel` + 修 3 个缺陷 + 改提示词文案 | **中高** | §6.2.1 四条单测全过（含多显示器往返）；现有坐标工具行为符合像素语义 |
| **P3** | `locate_element` 接入 B5（批量 ≤3） | 中 | 真实界面定位成功率可测；hint A/B 结果记录；失败能返回「找不到」 |
| **P4** | 补 7 个新动作 + 改 CLICK/SCROLL 语义 + `MouseButton::Middle` | 中 | 13 动作与 OSWorld 语义对齐单测 |
| **P5** | 删标注服务 + 9 个 `_index` 工具 + `reference_anchors` | 高 | 删除后所有保留工具测试全绿 |

**P5 必须最后执行**——它是唯一会降低现有能力的步骤。P3 验证通过前删掉，
中间状态就是「既没有 SoM，也没有可靠定位」。

---

## 10. 已确认决策

| # | 问题 | 结论 | 依据 |
|---|---|---|---|
| 1 | 第 17 个动作是什么？ | **不存在**。按 **13** 个动作实施 | `control/remote.rs` 的 `all_thirteen_actions_match_the_osworld_table`；OSWorld 源码只有 13 个动作 |
| 2 | `SCROLL` 改为像素 `dx`/`dy`？ | **已改**。`SCROLL_PIXELS_PER_LINE = 100`，本地实现把像素换算成行数，远端发 `hscroll`+`vscroll` | `input/actions.rs:65` |
| 3 | `DRAG_TO` 与现有 `drag_from_to` 并存？ | **并存**。`drag_from_to` 保留（拖拽已有元素），`mouse_drag_to` 新增（拖到绝对坐标），语义不同 | `tools/mod.rs:75` |
| 4 | `TYPING` 与现有 `input_focused` 并存？ | **并存**。`input_focused` 保留，`keyboard_type` 新增 | `tools/mod.rs:160` |
| 5 | OSWorld 评测时 Pointer 的身份 | **A：Pointer 当 agent**，`locate_element` 可作为组合工具。另跑一组 B 模式（只用 13 动作）作为可比基线 | 产品形态是「定位 + 动作组合」，砍掉定位等于不测核心能力 |
| 6 | P0 使用哪 20 张真实截图 | **已定**：一张冻结桌面（1512×982）上的 20 个目标，四组测试见附录 | `tests/locate_fixture/` |

> 第 5 条的两个数字要分开报告：A 模式（含 `locate_element`）是产品真实形态，
> B 模式（纯 13 动作）才能和其他 agent 横向比较。混在一起报没有意义。

---

## 附录：相关实测数据

### ScreenSpot-Pro ab100（100 条，qwen3.8-flash）

| 版本 | 命中 | 均值 | 备注 |
|---|---|---|---|
| B3 | 61.0% | 7.0s | 单次估计 + 裁片阶梯 |
| B4m | 74.0% | 22.2s | 旧最优 |
| **B5** | **79.0%** | 31.4s | 定位与校验解耦 |
| B6 | 60.0% | 50.2s | 光标点方案（已证伪） |
| B7c | 76.0% | 39.0s | 框 + 判中心 |
| B8 | 71.0% | 42.7s | 校验字段 6→2（已证伪） |

### M3 全量（1261 条）

| 版本 | 命中 |
|---|---|
| B3 | 83.19% |
| B4k | 86.76% |
| **B4m** | **88.42%** |
| B4v | 87.79% |

B5 未跑全量。

### 真实桌面 fixture 测试（P0，2026-10-21）

在冻结的真实 macOS 桌面截图（`crates/pointer-core/tests/locate_fixture/desktop_1512x982.png`，
1512×982 逻辑像素）上跑 20 个目标，覆盖菜单栏 / 状态栏 / Dock / 桌面四个区域。
真值 bbox 逐张裁剪确认后写入同目录 `truth.json`，图名、尺寸、案例、真值全部由该文件驱动，
代码里不硬编码任何案例。

```bash
cd crates/pointer-core
set -a; . tests/locate_fixture/env.example; set +a
cargo test --test locate_fixture -- --ignored --nocapture
```

#### 四组结果（qwen3.8-flash，思考关闭，单例 60s 超时）

| 测试 | 场景 | 结果 | 耗时 |
|---|---|---|---|
| `frozen_desktop_grounding` | 无 hint，全屏定位 | **20/20** | 222.7s |
| `frozen_desktop_hint_grounding` | hint 错位 3 个目标宽高 | **20/20** | 250.1s |
| `frozen_desktop_far_hint_stays_honest` | hint 在屏幕对角（1169–1736px） | **20/20 rescued**，0 自信错误 | 313.1s |
| `absent_targets_fail_honestly` | 目标不存在（5 例） | **5/5 诚实失败**，0 幻觉 | 163.1s |

#### 思考模式是负收益（A/B）

同一 fixture、同一模型，仅切换 `round_enable_thinking`：

| 指标 | 关思考 | 开思考 |
|---|---|---|
| 命中 | **20/20** | 18/20 |
| 墙钟 | **222.7s** | 361.1s |
| 平均每例 | **11.1s** | 18.0s |
| reasoning 标记 | **0** | 39 |
| 卡死超时 | **0** | 2 |

开思考的两次超时都源于 verify 纠错话术触发自我怀疑（"也许用户说错了"），
输出数千字 reasoning 后仍无结论。**`locate_element` 已固定 `round_enable_thinking = Some(false)`**，
与 B5 基准的 `reasoning_effort=none` 对齐。定位是视觉任务，不需要长推理。

#### hint 的代价

| 模式 | 命中 | 耗时 | 相对无 hint |
|---|---|---|---|
| 无 hint | 20/20 | 222.7s | — |
| 近距错 hint | 20/20 | 250.1s | +27s |
| 远距错 hint | 20/20 | 313.1s | +90s |

hint 的位置不影响搜索空间：重定位始终是**全屏** `locate_in(..., full, ..., Some(&hint))`，
hint 只作首轮猜测与纠错提示。因此**传错 hint 不会让结果变错，只会变慢**。
上游应当只在有依据时传 `hint_box`（上一帧位置、用户点过的坐标），不要"随便猜一个"。

#### 诚实性

不存在目标的 5 例全部 `ok=false` 且 `box=None`，走满 3 轮后放弃，无 fallback 坐标。
唯一值得留意的是「格式」菜单项耗时 44.3s（其他 28–32s）——**名字越接近真实元素，放弃得越慢**。

> 记录一次测试设计失误：初版把「微信」「VS Code」当作不存在样本，两轮都报幻觉；
> 裁剪后发现 Dock 里确实有这两个图标，是**用例错了，定位是对的**。
> 不存在样本必须经裁剪确认，不能凭印象。

### 关键结论

- **框的尺寸约束是精度来源**：B6（只输出点）比 B5 低 19pp，误差中位 17.0px vs 5.3px。
- **校验字段不是冗余**：B8 把 6 个必填字段压到 2 个，verified 率从 76% 升到 86%，
  但假确认从 14% 升到 20%，净 −5pp。逐字段填写是强制逐项核对的脚手架。
- **小样本结论不可信**：本会话已有 5 次「<20 条样本正收益 → 全量归零或倒退」。

---

[返回 Agent 专题](README.md) | [返回文档总索引](../README.md)
