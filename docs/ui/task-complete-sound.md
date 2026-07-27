# 任务完成提示音

对话**整轮**正常结束时（收到流事件 `done`），若用户偏好开启，则播放短促双音提示音。

## 用户设置

- 位置：设置 → **平台账户 / 管理员账户** → **通知** → **完成时播放提示音**
- 字段：`UserSettings.playSoundOnFinish`（camelCase JSON）
- 默认：`true`
- 持久化：`user_settings.json`（切换开关后立即 `update_user_settings`，无需页脚保存）
- 开启时可试听一次

## 实现要点

| 层 | 说明 |
|----|------|
| 前端播放 | `src/lib/taskCompleteSound.ts`（Web Audio API，桌面端与网页端通用） |
| 触发 | **仅** `handleDone`（`StreamEvent::Done`，整轮 `run_chat` 结束）；在 `finally` 中播放，避免落盘失败跳过提示音 |
| 条件 | 该会话 Done 前仍为 `generating`，**或**仍有未关闭的 turn timing（`hasActiveTurn`）。后者避免弱网对账提前清掉 generating 后漏播 |
| 音频解锁 | 用户点发送 / 设置里试听时 `primeTaskCompleteAudio()`；页签回到前台时尝试 `resume`；`closed` 时重建 AudioContext |
| 不触发 | 用户停止生成、流错误取消等非正常完成路径；重复 Done（turn 已关且非 generating） |

取消 / 报错结束不会播放提示音。

弱网下若 SSE 丢了 `done`，界面可能卡住执行中；见 [chat-stream-resync.md](../developer/chat-stream-resync.md)。
