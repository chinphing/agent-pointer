# MessageList 流式输出滚动跟随

流式输出时消息列表默认贴底跟随；用户一旦主动向上阅读，应停止自动下拽，直到用户回到底部或点击「滚动到底部」。

「滚动到底部」按钮仅在**已脱离底部跟随**或当前窗口还有更新内容（`hasMoreNewer`）时显示。around / 空洞窗口的贴底不是全文结尾，程序化 `toBottom` 不得把跟随打开并藏掉该按钮。内存里即使已有越过游标的直播行，只要 `hasMoreNewer`，绘制列表仍只含窗口，跟随保持关闭。

## 行为

| 状态 | 条件 | 自动滚动 |
|------|------|----------|
| 跟随 | 贴底（或刚点跳转按钮 / 切回已在尾部的会话），且 `!hasMoreNewer` | 新 token / 新消息继续滚到底 |
| 脱离 | 向上滚轮 / 手指下拖 / 距底超过阈值 / 停在 around 空洞窗口（`hasMoreNewer`） | 不再 `scrollToIndex` |
| 恢复 | 距底 ≤ 附着阈值，或点击跳转按钮 / 切走再回来（around 窗口会先换成尾部再贴底） | 重新跟随 |

切回**已在真实尾部**且仍在流式的会话时：不要因为直播行尚无 `position` 就
`jumpToLatest` / force 换尾（见 `hasDisconnectedLiveTail`）。只贴底跟随即可。

阈值（`MessageList.vue`）：

- **附着** `ATTACH_BOTTOM_PX = 8`：只有几乎贴底才恢复跟随（滞回，避免轻微上滑立刻被 `onScroll` 重新贴底）。
- **脱离** `DETACH_BOTTOM_PX = 48`：仅靠滚动**位置**脱离时的距离。
- **上滑即时脱离**：`scrollTop` 减小（滚轮 / 触控）时立刻 `followOutput = false`。对话列表无可见滚动条，避免与右缘导航叠在一起。

程序化 `toBottom` 期间用短时 suppress 窗口忽略滚动事件，避免把跟随状态写乱。
流式高频贴底会**延长**同一窗口，而不是叠 depth 计数——否则整段生成期间 `onScroll`
都进不了脱离逻辑，看起来像「运行中无法往下滚」。贴底后同步 `lastScrollTop`，
否则首次拖动无法识别为上滑。

流式时末轮高度常比估算值长：每次 live 渲染信号要 `resizeItem` 末轮，
否则 `getTotalSize` 偏短，内容被 scroller 裁切，滚轮到头也看不到底部。

贴底实现要点：

1. **真底部**：`scrollToIndex(align: 'end')` 只对齐最后一行，不会把
   virtualizer `paddingEnd` / scroller `pb-*` 滚进视口；随后必须
   `scrollTop = scrollHeight - clientHeight`。
2. **切换/挂载 settle**：会话切换会 remount 列表，行高先用估算值；
   `toBottom({ settle: true })` 在随后两帧再贴一次，减少测量校正后的下跳与裁切。
3. **视口变矮**：scroller `ResizeObserver` 侦测 `clientHeight`（Composer / 草稿增高、窗口变矮）。
   **无论是否跟随**都记下高度，并按视口差值补偿 `scrollTop`，让当前画面里的消息留在原处
   （读最后两行或中间历史时打字，底栏都不会盖住正文）。搜索定位期间只记高度、不补滚动。
   视口变高（删草稿）不补，避免把内容往下拽。同时观察列表根节点：WebKit 有时只通知 flex 父级变矮、不通知 `h-full` scroller。
4. **总高度变化**：跟随态下 virtualizer `getTotalSize()` 变化时再贴一次真底部
   （覆盖切换后 estimate→measure 与末轮展开）。输入框刚把视口变矮之后**一直跳过**这次贴底，直到视口重新变高（删行）或输入框失焦；不要只靠约 120ms 时间窗——WebKit 往往更晚才量行高，贴底会把最后几行拽进 Composer。正在输入框里打字时同样跳过。
5. **内容矮于视口**：回合从顶部排起，空白留在消息与输入框之间。
   不要用弹性空白把短对话顶到输入框上方（新会话会像贴在底部）。

同约定也用于：

- 终端实时输出弹层（`TerminalLiveOutputModal.vue`）
- 思考过程预览框（`ModelThoughtPanels.vue`）

## 实现

- `src/components/chat/MessageList.vue`
- `src/lib/messageListScrollFollow.ts` — `nextFollowOutputAfterScroll`、视口变矮补偿与 totalSize 贴底抑制、程序化滚动 suppress 窗口
