# Computer 子 Agent 提示词结构

内置 **computer** Agent 的系统提示由三部分拼接而成（磁盘覆盖 Agent 目录时行为一致）：

1. **`COMMUNICATION_SHARED.md`**（若存在）— 桌面截图槽位、原图/标注图/放大图。
2. **`COMMUNICATION.md`**（**运行时 slim**）— **Ground rules**；五段前缀 **`Pointer:`** → **`Verify:`** → **`Repetition:`** → **`Next:`** → **`Location:`**（无屏目标时省略）；**Verify** 三态 **`VERIFIED` / `NFO` / `FAILED`**（**无 `PARTIAL`**）；**`NFO`** 仅表示需进一步验证、**无 NFO 锁**；含 **Required form**、**Mini example**、**Full chain**（完整 **`<response>`** XML：五段推理写在 **`<thoughts>`** 内，**`tool_args`** 可用 **`...`** 占位；**`<headline>`** 保持简短）。
3. **`AGENT.md` 正文** — 角色与循环要点。

更长展开与历史完整段落见 **`COMMUNICATION_FULL.md`**（**不**随运行时加载，可作编辑参考）。

**最近动作注入：** mouse / hotkey / composite_action / modified_click / wait / clipboard 成功后写入 `VisionState`；`[CUR_SCREEN]` 末尾最多 5 条 **`[Recent desktop tool calls]`**。部分回合的 **`[CUR_SCREEN]`** 文本会在槽位说明之后附带 **Pointer neighbor reference bboxes**：先在以指针为中心的 **300×300 px**（与指针放大图同尺度）截图窗口内筛「框中心落在窗内」的标注，再在其中按到指针距离取最多 5 个；若窗内无框则提示模型可直接用坐标类方法对准可见目标、**不依赖**锚点列表（**不会**再用全图远距离框回填）。仅供 **坐标类** 方法作锚点，**不能**当作 overlay **`index`** 点击目标。剪贴板 XML 请写 **`clipboard:read`** / **`clipboard:write`**（运行时仍接受旧方法名 **`read_clipboard`** / **`write_clipboard`** 作为别名）。
