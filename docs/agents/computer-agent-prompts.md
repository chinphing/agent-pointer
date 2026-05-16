# Computer 子 Agent 提示词结构

内置 **computer** Agent 的系统提示由三部分拼接而成（磁盘覆盖 Agent 目录时行为一致）：

1. **`COMMUNICATION_SHARED.md`**（若存在）— 桌面截图槽位、原图/标注图/放大图。
2. **`COMMUNICATION.md`**（**运行时 slim**）— **Ground rules**；五段前缀 **`Pointer:`** → **`Verify:`** → **`Repetition:`** → **`Next:`** → **`Location:`**（无屏目标时省略）；**Verify** 三态 **`VERIFIED` / `NFO` / `FAILED`**（**无 `PARTIAL`**）；**`NFO`** 仅表示需进一步验证、**无 NFO 锁**；含 **Required form**、**Mini example**、**Full chain**（完整 **`<response>`** XML：五段推理写在 **`<thoughts>`** 内，**`tool_args`** 可用 **`...`** 占位；**`<headline>`** 保持简短）。
3. **`AGENT.md` 正文** — 角色与循环要点。

更长展开与历史完整段落见 **`COMMUNICATION_FULL.md`**（**不**随运行时加载，可作编辑参考）。

**`Location:` 行约定（overlay，共 4 行）：** **先证据后结论**。**第 1 行** **Placement→frame** → target / bbox。**坐标路由**：优先 **`[Zoom pointer after action]`**（与 **Pointer position** 同心的 300×300 标注放大图）作视觉锚点推导 **(x,y)**；目标不在指针 zoom 内则回退全屏或 line 1 帧。**index 路由**：**第 2 行 (a)(b)(c) → therefore index N**。

**防捏造上一步：** **`Verify:`** 必须先写 **`Last automated step:`**；无历史则 **`none`**，禁止编造上一步。**`COMMUNICATION.md`** 样例覆盖 **click / coordinates / scroll / wait / hotkey / response** 等，**`clipboard:*`** 仅见工具 prompt 与 **§ Off-frame tools (rare)** 规则，勿默认走剪贴板。

**最近动作注入：** mouse / hotkey / composite_action / modified_click / wait / clipboard 成功后写入 `VisionState`；`[CUR_SCREEN]` 末尾最多 5 条 **`[Recent desktop tool calls]`**。部分回合在槽位说明后附带 **Pointer position**（整幅 capture 像素 / 0–1000）与 **Pointer coordinate anchor**：用 **`[Zoom pointer after action]`**（与指针同心的 **300×300 px** 标注放大图）作**坐标类**视觉锚点，再映射到 session **x/y**；目标不在指针 zoom 内则回退全屏或 Location 所选帧。不能当作 overlay **`index`** 点击目标。剪贴板 XML 请写 **`clipboard:read`** / **`clipboard:write`**（运行时仍接受旧方法名 **`read_clipboard`** / **`write_clipboard`** 作为别名）。
