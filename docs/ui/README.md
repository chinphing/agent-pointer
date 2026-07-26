# 界面与前端（ui）

| 文档 | 说明 |
|------|------|
| [visual-theme.md](visual-theme.md) | 扁平主题 token、深浅色切换 |
| [task-complete-sound.md](task-complete-sound.md) | 任务完成提示音（账户设置 `playSoundOnFinish`） |
| 设置 → **平台账户 / 管理员账户** | 登录状态、「退出登录」；完成提示音开关 |
| [assistant-message-ui.md](assistant-message-ui.md) | 助手消息 `thoughts` / reasoning / 原始输出面板等约定 |
| [workspace-file-preview-find.md](workspace-file-preview-find.md) | 右侧工作区文本预览查找（⌘/Ctrl+F、上下匹配） |
| 侧边栏会话搜索 | 第二行展示命中关键词附近的 snippet；点击正文命中结果会定位到对应消息并短暂高亮。详见 [`../internals/sidebar-conversation-search.md`](../internals/sidebar-conversation-search.md) |
| 新会话空状态 | 「热门」与分类并列 Tab（`GET /api/experiences/home`）；搜索关键词（`GET /api/experiences?q=`）；点击后切换绑定智能体并将 `prompt_text` 填入输入框 |
| 经验封面图 | 欢迎页卡片分两栏：标题栏（左标题 + 右 **24×24** 徽章，垂直居中对齐）；内容栏（正文独占整行）；**推荐经验**整块默认收缩，点击标题栏展开；上传最小 **200×200** 像素，建议 1:1 |
| 经验卡片字数 | 标题最多 **15** 字、说明（`narrative_text` / excerpt）最多 **45** 字；与官网创建/编辑表单及 API 校验一致（`experience_card_limits`） |

[返回文档总索引](../README.md)
