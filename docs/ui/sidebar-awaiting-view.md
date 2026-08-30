# 侧栏「完成未查看」标记

后台会话跑完后，若用户当时不在看该会话，侧栏把消息图标换成**实心圆点**，直到点开该会话。会话仍在生成、或仍有后台子任务时，优先显示转圈，不显示圆点。

## 规则

| 时机 | 行为 |
|------|------|
| `Done` 且当时 `currentId` 不是该会话 | 标记待查看（与完成提示音同一条件：曾 generating 或有 active turn；后台任务仍在跑时不响提示音） |
| 打开 / 切到该会话 | 清除标记 |
| 该会话再次开始生成，或有后台子任务 | 清除标记，显示转圈 |
| 删除会话 | 清除标记 |

仅内存，刷新后不保留。实心点带轻柔闪动（`sidebar-awaiting-dot`）。

## 相关代码

- `src/stores/chat.ts` — `awaitingViewIds` / `isConversationAwaitingView`
- `src/stores/chat/streamHandlers/sessionHandlers.ts` — `handleDone`
- `src/components/layout/AppShell.vue` — 侧栏图标
