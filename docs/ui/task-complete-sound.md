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
| **桌面（Tauri）** | 原生播音：`play_task_complete_chime` 按**原 Web Audio 四层音色**离线合成 WAV（G3/G4 + D4/D5、1400Hz 低通、master 1.45），经 OS 播放（macOS `afplay` / Windows `SoundPlayer` / Linux `paplay\|aplay\|ffplay`）。不走 WebView，避免无声 |
| **网页** | 同款合成 WAV + `HTMLAudioElement`；发送时 `primeTaskCompleteAudio` 静音解锁 |
| 触发 | **仅** `handleDone`（`StreamEvent::Done`）；在 `finally` 中播放 |
| 条件 | Done 前仍为 `generating`，**或**仍有未关闭的 turn timing（`hasActiveTurn`） |
| 不触发 | 用户停止生成、流错误取消等；重复 Done（debounced） |

取消 / 报错结束不会播放提示音。

弱网下若 SSE 丢了 `done`，界面可能卡住执行中；见 [chat-stream-resync.md](../developer/chat-stream-resync.md)。
