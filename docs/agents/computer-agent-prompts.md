# Computer 子 Agent 提示词结构

内置 **computer** Agent 的系统提示由三部分拼接而成（磁盘覆盖 Agent 目录时行为一致）：

1. **`COMMUNICATION_SHARED.md`**（若存在）— 桌面截图槽位、原图/标注图/放大图（含 **`[Zoom pointer before action]`**）。
2. **`COMMUNICATION.md`**（**运行时 slim**）— **Ground rules**；六段 **`Pointer:`** → **`Verify:`** → **Repetition:** → **`Next:`** → **`Location:`** → **`Tool route:`**；**Pointer** 几何以 **`[Zoom pointer before action]`**（**4×**、±**50 px** 裁剪）为准；**Verify** 固定顺序：**`Last automated action:`** → **Before vs after**（唯一读图）→ **Clear evidence**（复述 before/after 的**画面 UI 结果**；禁止 click/无报错/点中等动作叙事）→ **Action type** → **Mouse judgment** → **`Lookup`（三键）→ `Match`（12 行 Verify→Step result 表）→ `Step result` + `Cause`（须与 Match 一致）**；**Next** line 1 再复述 Verify 结论后做 **Verify→Next** 的 Lookup/Match；含 **Required form**、**Mini example**、**Full chain**。
3. **平台快捷键补充段**（运行时按目标平台自动追加）— 用于约束 `hotkey` 的主修饰键与常见组合（macOS / Windows / Linux 分开维护）。
4. **`AGENT.md` 正文** — 角色与循环要点。

更长展开与历史完整段落见 **`COMMUNICATION_FULL.md`**（**不**随运行时加载，可作编辑参考）。

**阶段分工：** **`Next:`** 两行——line 1 顺序：**先复述 Verify/Repetition 结论** → **Lookup**（用该结论作键查 Verify→Next 表）→ **Match**（命中行）→ **this turn**（下一步）；line 2 全屏目标与 Match 行一致，**不选工具**。**`Location:`** 只做 overlay 分析（frame / bbox / index N / single|multiple / 坐标几何），**不写工具名**。**`Tool route:`** 汇总 `Location` 结论并给出本轮明确工具调用；**`Tool route:`** 第 2 行与根 **`tool_name`** 必须一致。非 overlay 操作用 **`Location: n/a`** + **`Tool route:`**（如 hotkey / wait）。

**`Location:` 行约定（overlay，最多 4 行）：** **第 1 行** 含 **`traits inside that bbox:`**（`distinct controls` + `wrap count` + `intended sub-target`），**禁止** line 1 出现 overlay 编号。**第 2–4 行必须以 traits 为准**：line 3 的 single/multiple 只引用 traits 的 wrap count，不能用任务动作名代替。**第 2–4 行** index → exclusivity（来自 traits）→ 可选坐标几何。

**`Tool route:`（2 行）：** line 1 **`Next recap: this turn: …`**（与 Next line 1 的 `this turn:` 子句一致）+ **`Location recap:`**；line 2 选工具。

**防捏造上一动作：** **`Verify:`** 必须先写 **`Last automated action:`**；无历史则 **`none`**，禁止编造上一动作。**`COMMUNICATION.md`** 样例覆盖 **click / coordinates / scroll / wait / hotkey / response** 等，**`clipboard:*`** 仅见工具 prompt 与 **§ Off-frame tools (rare)** 规则，勿默认走剪贴板。

**`[CUR_SCREEN]` 图像顺序（有上一轮时）：** (1) **`[Screen before action]`** — 上一轮 unmarked 全屏 + 当前合成指针；(2) **`[Zoom pointer before action]`** — 自 before 帧指针 **±50 px** 裁剪并 **4×** 放大（**Pointer / Verify 鼠标几何标准**）；(3) **`[Screen after action]`** … (7) **`[Zoom pointer after action]`**。首轮无 (1)(2)。

**最近动作注入：** mouse / hotkey / composite_action / modified_click / wait / clipboard 成功后写入 `VisionState`；`[CUR_SCREEN]` 末尾最多 5 条 **`[Recent desktop tool calls]`**。部分回合在槽位说明后附带 **Pointer position**（整幅 capture 像素 / 0–1000）与 **Pointer coordinate anchor**：**坐标类** `*_at` 仍以 **`[Zoom pointer after action]`**（**300×300 px** 标注放大）为锚点；**Pointer:** 判热点与中心用 **`[Zoom pointer before action]`**（有则必写）。不能当作 overlay **`index`** 点击目标。剪贴板 XML 请写 **`clipboard:read`** / **`clipboard:write`**（运行时仍接受旧方法名 **`read_clipboard`** / **`write_clipboard`** 作为别名）。
