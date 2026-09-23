# 工具正文的前端内存

WebView 在一轮任务里涨得快，是因为当前用户回合把工具全文留在内存里。虚拟列表按回合分页，跟随底部输出时不会裁这一轮。

模型用的截断结果、SQLite 里的全文都不变。这里只改前端 store 里留什么。

## 原则

- 进行中的工具保留流式所需的缓冲，并且有上限。
- 工具离开 `running`（成功、失败、取消、拒绝），并且宿主消息已经在数据库里之后，只留折叠行和本轮变更统计要用的字段。
- 用户展开某一行时再取那一行的参数和结果，关上再卸掉。同一会话同时只留一份展开正文。
- 裁剪只发生在内存。已经落库的消息不再 `persistAppend`。不要把裁过的工具调用写回数据库。
- 主会话和子 Agent 转录用同一套规则。

回合级裁剪（10 分钟 / 至少 8 个用户回合）继续负责整段历史进出内存，不负责卸掉当前回合里的文件正文。见 [message-turn-pagination.md](message-turn-pagination.md)。

## 终端输出

设置项 `terminalOutputMaxBytes`（默认 16 KiB，范围 4 KiB–256 KiB）同时约束三份数据：

| 副本 | 行为 |
|------|------|
| 回给模型的 `result.stdout` / `result.stderr` | 已是尾部截断，不变 |
| 进行中的 `toolCall.terminalOutput` | 每次追加后只留同样长度的尾部。超出时前缀 `...[output truncated]\n`，与模型结果一致 |

直播弹窗读 `terminalOutput`，只在 `running` 时存在，命令结束时关闭。截断做在 `applyToolOutputDeltaBatch`，主会话和子 Agent 共用。

命令结束后，`terminalOutput` 不再单独处理。它和参数、结果一样属于正文，由下一节的一次判断决定整组去留。

## 工具正文

正文是一组字符串，名单只写在裁剪函数里：`arguments`、`result`、`terminalOutput`、`webSearchOutput`。调用处不按字段名分支。

每个工具调用只有一个内存标记 `bodyEvicted`（只放内存，不写入数据库）：

- `running` / `pending` / `pending_approval`：不裁。`terminalOutput` 仍按上一节收成尾部。
- 已经结束，且每个正文字符串的 `length`（UTF-16 码元）都 **≤ 4096**：正文留下，`bodyEvicted` 为 `false`。缺省的字段算 0。
- 已经结束，且任一正文超过 4096：整组清空，`bodyEvicted` 为 `true`。`arguments` 置成空字符串，其余正文删掉。

整组进出，避免「这个字段卸了、那个还在」。展开要么直接用内存里的正文，要么整组从数据库取回。

折叠行只用 `displayLabel`、`displaySummary`、状态、耗时、错误。这些字段不在正文名单里，裁剪不动它们。后端没给 `displaySummary` 时，清空前把文件路径或参数摘要写进 `displaySummary`，折叠行才不会在正文卸掉后变成空白。`bodyEvicted` 为 `true` 时不要再解析 `arguments` 或 `result`。

本轮「修改了 N 个文件」在清空正文之前读完 `result.stats`。只有没有 `stats` 的旧写入才从 `arguments.content` 数行数，数完只留整数。

`role = tool` 的行不展示正文。进入 store 时丢掉 `content`，保留 `id`、`role`、`toolCallId`、`position`。不要按字段做标记，也不要把这些行从列表里删掉。

## 水合

最近 8 个回合从 SQLite 载入后、写进 store 之前，对每条非 `running` 工具做上面的裁剪。子 Agent 的 `assignLoaded` 同样处理。否则重新打开会话会把全文再载回来。

## 展开再取

用户点开一行时只看 `bodyEvicted`。不是 `true` 就用内存里的正文。是 `true` 才向数据库要这一次调用的整组正文：

1. 用这一行已经知道的宿主消息 id 调用 `load_message`，取出该 `toolCallId` 的正文并写回，`bodyEvicted` 置为 `false`。宿主消息上的其它工具调用立刻丢掉。
2. 行再次折叠时重新跑上一节的判断。
3. 打开另一行时，先把上一份展开正文重新裁掉。

不要用 `load_assistant_message_with_tool_call`。它会按内容扫描并解码多行。`load_tool_message_by_call_id` 只返回 `role = tool` 的结果行，没有写入参数。

尚未落库、内存里仍是全文的进行中工具，展开直接读内存，不发请求。

主会话的助手消息要等这一轮 `message_end` 才在数据库里。在那之前不卸正文，否则展开时还取不回。`message_end` 先把宿主 id 记成已落库，再卸掉这条消息上已结束的工具。之后才结束的后台工具，因为宿主 id 已经在已落库集合里，状态落地时就可以卸。子 Agent 同一条规则。重新打开会话时，页数据本来就来自数据库，水合时直接卸。

## 不改

- 模型上下文、工具上限的设置含义、SQLite 里的消息全文。
- 工作区终端自己的 5000 行缓冲。
- 助手回复的 `content` / `reasoning`。那是另一项，这里不裁。

对话内查找只搜已留在内存里的文本，看不到已卸掉的工具正文。侧栏搜索仍查数据库。

## 实现

- `src/lib/toolCallBody.ts` — 正文一组的上限、裁剪、取回
- `src/stores/chat.ts` — 终端尾部截断；宿主落库后裁剪；水合入口；展开时 `loadConversationMessage`
- `src/components/chat/ToolCallRow.vue` — 展开时加载、折叠时卸掉
- `src/stores/chat/terminalLive.ts` — 弹窗在 `running` 期间读已截断的尾部

子 Agent 行在写进 scoped store 之前走同一套水合裁剪。持久化克隆会丢掉 `bodyEvicted` 和 `fileChange`，这两项不进数据库。

桌面和 Web 都走 store 这条路径，两端一起改。
