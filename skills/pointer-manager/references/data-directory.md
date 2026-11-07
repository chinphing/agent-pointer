# PointerApp 数据目录

根目录：**`{data_dir}/PointerApp/`**（`{data_dir}` 见主技能「数据目录根路径」表）。

相关但**不在**此目录下：

| 路径 | 说明 |
|------|------|
| `~/.pointer/skills/` | 用户 Skill 库（导入/自建，可改） |
| `~/.pointer/external_skills_probe_done` | 外部 Skill 首次探测标记 |

---

## 目录与文件一览

| 路径（相对 PointerApp/） | 简介 | 可否清理 |
|--------------------------|------|----------|
| `conversations.db` | 对话历史（SQLite WAL） | ❌ 核心数据 |
| `user_settings.json` | 用户设置（主题、已启用 Skill 等） | ❌ |
| `local_platform_settings.json` | 智能体/界面偏好（桌面端） | ❌ |
| `auth.dat` / `key.dat` | 平台 OAuth 加密会话 | ❌ 删后需重新登录 |
| `.env` | 终端工具自动加载的用户环境变量 | 按需编辑 |
| `logs/` | 运行时日志目录 | 见下节 |
| `logs/pointer_*.log` | 应用日志，按日轮转，保留约 7 个文件 | ✅ 可删旧文件腾空间 |
| `logs/llm_prompts/{会话ID}/` | 调试分区开启「保存每轮对话请求」后的 LLM 请求落盘；按会话分子目录 | ✅ 可整目录清理 |
| `computer-captures/` | 电脑操控调试截图；**仅调试分区开启「保存截图到本地」时**才会写入 | ✅ 可删；启动时自动清理 7 天前 |
| `conversation-media/` | 会话附件与聊天媒体缓存 | ⚠️ 删后部分附件需重新获取 |
| `generated-media/` | AI 生成图片/视频等输出 | ⚠️ 按需清理 |
| `memories/` | 持久化记忆存储 | ❌ 除非用户明确要求清空记忆 |
| `session-sandboxes/` | 每个会话的隔离工作目录（`_anonymous`、会话 ID、项目名） | ⚠️ 删后该会话的工作区文件丢失 |
| `turn-baselines/` | 每轮改动前的文件内容快照，供「变更审阅」对比 diff | ✅ 可删；仅影响历史 diff 展示 |
| `worktrees/` | 项目托管 git worktree | ❌ 删后需重新创建 |
| `agents/` | 用户自定义智能体覆盖（`AGENT.md` / `COMMUNICATION.md`） | ❌ 有内容时不可删 |
| `skills/` | 应用内置 Skill 同步副本（带 `.bundled_manifest`） | ❌ 由应用维护 |
| `native/` | 打包释放的原生扩展二进制 | ❌ 由应用维护 |
| `task_boards.db` | 任务板 SQLite | ❌ 除非用户要清空任务板 |
| `token_usage.db` | Token 用量统计 | ❌ |
| `token_usage_archives/` | 用量库归档 | ⚠️ 按需清理 |
| `platform_model_catalog.json` | 平台下发的模型目录缓存 | ✅ 可删，重启后重新拉取 |
| `channels_config.json` | IM 通道配置 | ❌ |
| `channel_credentials/` | 通道凭证 | ❌ |
| `channel_pairing/` | 通道配对状态 | ❌ |
| `channel_histories.deprecated/` | 旧版 IM JSON 历史（仅老版本升级后残留） | ⚠️ 可删 |
| `work_items.db` / `pointer.db` / `PointerApp.db` | 旧版本残留，当前代码不再读写 | ✅ 可删 |

---

## 日志管理

**查看**

- 最新日志：`logs/` 下按修改时间排序，优先 `pointer_*.log`
- macOS 示例：`~/Library/Application Support/PointerApp/logs/`

**清理建议**

1. 先 `file_list` 确认文件大小与数量
2. 可安全删除：`logs/pointer_*.log` 中较旧文件、整个 `logs/llm_prompts/`（仅调试产物）
3. 删后应用会继续写入新日志；无需重启（除非用户遇到异常）
4. 不要删除正在写入的当日日志，除非用户确认并已退出应用

**开启 LLM 提示词落盘**

- 设置 → **调试** 分区 →「**保存每轮对话请求**」开启后才会产生 `logs/llm_prompts/{会话ID}/`
- 调试分区仅对平台管理员 / 独立部署账号显示

---

## 电脑操控截图

- **前提**：设置 → **调试** 分区 →「**保存截图到本地**」已开启；关闭时不会产生新文件
- 路径：`computer-captures/{YYYY-MM-DD}/{conversation_id}/`
- 应用启动时会删除 **7 天前** 的日期目录
- 用户可手动删除整个 `computer-captures/` 释放空间；不影响对话记录，仅丢失本地调试截图

---

## 操作原则

- 只读说明：直接引用上表，给出完整绝对路径
- 删除/清空：列出将删除的路径模式，说明后果，等用户确认
- 核心库（`conversations.db`、`auth.dat`、`user_settings.json`）**禁止**擅自删除或覆盖
