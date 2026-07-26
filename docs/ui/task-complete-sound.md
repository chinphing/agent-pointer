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
| 触发 | **仅** `handleDone`（`StreamEvent::Done`，整轮 `run_chat` 结束） |
| 不触发 | `message_end` 后的 `maybeFinishGenerating` 兜底（它也会在工具轮次间隙触发，不能当完成音） |
| 不触发 | 用户停止生成、流错误取消等非正常完成路径 |

取消 / 报错结束不会播放提示音。
