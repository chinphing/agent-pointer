# Computer 子 Agent 提示词结构

内置 **computer** Agent 的系统提示由三部分拼接而成（磁盘覆盖 Agent 目录时行为一致）：

1. **`COMMUNICATION_SHARED.md`**（若存在）— 桌面截图槽位、原图/标注图/放大图（含 **`[Zoom pointer before action]`**）。
2. **`COMMUNICATION.md`**（**运行时 slim**）— **Ground rules**；五段 **`Pointer:`** → **`Verify:`** → **Repetition:** → **`Next:`** → **`Location:`**；**Pointer** 几何以 **`[Zoom pointer before action]`**（**4×**、±**50 px** 裁剪）为准；**Verify** 固定顺序：**`Last automated action:`** → **Before vs after**（唯一读图）→ **Clear evidence**（复述 before/after，不再读图）→ **Action type** → **Mouse judgment** → **12 行查表** → **`Step result` + `Cause`**；**Next** 与结论 1:1；含 **Required form**、**Mini example**、**Full chain**。
3. **`AGENT.md` 正文** — 角色与循环要点。

更长展开与历史完整段落见 **`COMMUNICATION_FULL.md`**（**不**随运行时加载，可作编辑参考）。

**`Location:` 行约定（overlay，共 4 行）：** **先证据后结论**。**第 1 行** **Placement→frame** → target / bbox。**坐标路由**：优先 **`[Zoom pointer after action]`**（与 **Pointer position** 同心的 300×300 标注放大图）作视觉锚点推导 **(x,y)**；目标不在指针 zoom 内则回退全屏或 line 1 帧。**index 路由**：**第 2 行 (a)(b)(c) → therefore index N**。

**防捏造上一动作：** **`Verify:`** 必须先写 **`Last automated action:`**；无历史则 **`none`**，禁止编造上一动作。**`COMMUNICATION.md`** 样例覆盖 **click / coordinates / scroll / wait / hotkey / response** 等，**`clipboard:*`** 仅见工具 prompt 与 **§ Off-frame tools (rare)** 规则，勿默认走剪贴板。

**`[CUR_SCREEN]` 图像顺序（有上一轮时）：** (1) **`[Screen before action]`** — 上一轮 unmarked 全屏 + 当前合成指针；(2) **`[Zoom pointer before action]`** — 自 before 帧指针 **±50 px** 裁剪并 **4×** 放大（**Pointer / Verify 鼠标几何标准**）；(3) **`[Screen after action]`** … (7) **`[Zoom pointer after action]`**。首轮无 (1)(2)。

**最近动作注入：** mouse / hotkey / composite_action / modified_click / wait / clipboard 成功后写入 `VisionState`；`[CUR_SCREEN]` 末尾最多 5 条 **`[Recent desktop tool calls]`**。部分回合在槽位说明后附带 **Pointer position**（整幅 capture 像素 / 0–1000）与 **Pointer coordinate anchor**：**坐标类** `*_at` 仍以 **`[Zoom pointer after action]`**（**300×300 px** 标注放大）为锚点；**Pointer:** 判热点与中心用 **`[Zoom pointer before action]`**（有则必写）。不能当作 overlay **`index`** 点击目标。剪贴板 XML 请写 **`clipboard:read`** / **`clipboard:write`**（运行时仍接受旧方法名 **`read_clipboard`** / **`write_clipboard`** 作为别名）。
