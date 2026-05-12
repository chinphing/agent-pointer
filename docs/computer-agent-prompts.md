# Computer 子 Agent 提示词结构

内置 **computer** Agent 的系统提示由三部分拼接而成（磁盘覆盖 Agent 目录时行为一致）：

1. **`COMMUNICATION_SHARED.md`**（若存在）— 桌面截图槽位、原图/标注图/放大图的来源与用途。
2. **`COMMUNICATION.md`** — **内部**四段推理链（verify / repetition / next action / target location）；**`<thoughts>`** 仅简短线上摘要（与公共约定一致）。
3. **`AGENT.md` 正文**（YAML 头之后）— 角色与操作要点（slim）。

内置包通过 `include_str!` 将 shared 与 `COMMUNICATION.md` 合并；外部 Agent 目录若提供 `COMMUNICATION_SHARED.md`，加载时会自动前置合并。

**完整版占位：** 仓库中的 `COMMUNICATION_FULL.md`、`AGENT_BODY_FULL.md` 仅作文档与后续接入用，当前运行时不会加载。

**最近动作注入：** 成功执行桌面类工具（mouse / hotkey / composite_action / modified_click / wait）后，会把 `goal` / `action` 写入 `VisionState`；下一轮 `[CUR_SCREEN]` 用户消息末尾附带最多 5 条，供 repetition 判断（不依赖跨轮 overlay 序号）。
