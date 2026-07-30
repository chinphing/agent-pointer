# MessageList 流式输出滚动跟随

流式输出时消息列表默认贴底跟随；用户一旦主动向上阅读，应停止自动下拽，直到用户回到底部或点击「滚动到底部」。

## 行为

| 状态 | 条件 | 自动滚动 |
|------|------|----------|
| 跟随 | 贴底（或刚点跳转按钮 / 切换会话） | 新 token / 新消息继续滚到底 |
| 脱离 | 向上滚轮 / 手指下拖 / 距底超过阈值 | 不再 `scrollToIndex` |
| 恢复 | 距底 ≤ 附着阈值，或点击跳转按钮 | 重新跟随 |

阈值（`MessageList.vue`）：

- **附着** `ATTACH_BOTTOM_PX = 8`：只有几乎贴底才恢复跟随（滞回，避免轻微上滑立刻被 `onScroll` 重新贴底）。
- **脱离** `DETACH_BOTTOM_PX = 48`：仅靠滚动位置脱离时的距离。

程序化 `toBottom` 期间用 `programmaticScrollDepth` 忽略滚动事件，避免把跟随状态写乱。

贴底实现要点：

1. **真底部**：`scrollToIndex(align: 'end')` 只对齐最后一行，不会把
   virtualizer `paddingEnd` / scroller `pb-*` 滚进视口；随后必须
   `scrollTop = scrollHeight - clientHeight`。
2. **切换/挂载 settle**：会话切换会 remount 列表，行高先用估算值；
   `toBottom({ settle: true })` 在随后两帧再贴一次，减少测量校正后的下跳与裁切。
3. **视口变矮**：`ResizeObserver` 在跟随态下侦测 scroller `clientHeight`
   （Composer / ChangeSummary / 草稿增高），再 `scheduleToBottom`。
4. **总高度变化**：跟随态下 virtualizer `getTotalSize()` 变化时再贴一次真底部
   （覆盖切换后 estimate→measure 与末轮展开）。

同约定也用于：

- 终端实时输出弹层（`TerminalLiveOutputModal.vue`）
- 思考过程预览框（`ModelThoughtPanels.vue`）

## 实现

`src/components/chat/MessageList.vue`
