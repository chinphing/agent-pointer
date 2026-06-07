# 界面与前端（ui）

| 文档 | 说明 |
|------|------|
| [visual-theme.md](visual-theme.md) | 扁平主题 token、深浅色切换 |
| 设置 → **平台账户**（仅桌面端） | 平台登录状态、昵称、「退出登录」 |
| [assistant-message-ui.md](assistant-message-ui.md) | 助手消息 `thoughts` / reasoning / 原始输出面板等约定 |
| 新会话空状态 | 「热门」与分类并列 Tab（`GET /api/experiences/home`）；搜索关键词（`GET /api/experiences?q=`）；点击后切换绑定智能体并将 `prompt_text` 填入输入框 |
| 经验封面图 | 固定高度卡片：顶栏全宽标题（超长截断），底栏正文 `float` 环绕右下角封面（约 72×72px，在「点击使用」上方）；上传最小 **200×200** 像素，建议 1:1 |
| 经验卡片字数 | 标题最多 **48** 字、说明（`narrative_text` / excerpt）最多 **120** 字；与官网创建/编辑表单及 API 校验一致（`experience_card_limits`） |

[返回文档总索引](../README.md)
