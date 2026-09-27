# 界面与前端（ui）

| 文档 | 说明 |
|------|------|
| [i18n.md](i18n.md) | 界面中英国际化：`uiLocale`、vue-i18n、locale 文件约定与分期进度 |
| [visual-theme.md](visual-theme.md) | 扁平主题 token、深浅色切换；对话列宽跟中间栏（含 Workspace 拖拽）自适应 |
| [markdown-typography.md](markdown-typography.md) | 聊天 / 工作区 Markdown 正文字号、标题层级、加粗当标题 |
| [external-links.md](external-links.md) | 应用内 http(s) 链接用系统默认浏览器打开（桌面）/ 新标签（Web） |
| [markdown-charts.md](markdown-charts.md) | Markdown `chartjs` fence → 本地 Chart.js 交互图表 |
| [markdown-mermaid.md](markdown-mermaid.md) | Markdown `mermaid` fence → 本地 Mermaid，配色跟深浅色 token |
| [markdown-svg.md](markdown-svg.md) | Markdown `svg` fence → 消毒后内联流程图/示意图 |
| [markdown-media-boundaries.md](markdown-media-boundaries.md) | Markdown / SVG / Chart / HTML 功能边界与 HTML 表约定 |
| [task-complete-sound.md](task-complete-sound.md) | 任务完成提示音（账户设置 `playSoundOnFinish`） |
| [turn-elapsed.md](turn-elapsed.md) | 回合「工作耗时」；未开「默认收缩执行过程」时与增设前行为一致 |
| [web-branding-welcome-elapsed.md](web-branding-welcome-elapsed.md) | Server 可定制欢迎 tip / 耗时前缀（默认文案不动） |
| [sidebar-conversation-select.md](sidebar-conversation-select.md) | 侧栏点选：先提交 `currentId`，主区异步加载 |
| [last-conversation-restore.md](last-conversation-restore.md) | 启动恢复上次选中会话（`pointer.chat.lastConversationId`） |
| [sidebar-awaiting-view.md](sidebar-awaiting-view.md) | 后台完成未查看：侧栏实心圆点 |
| 设置 → **账户** | 余额、登录状态、「退出登录」；完成提示音开关 |
| [assistant-message-ui.md](assistant-message-ui.md) | 助手消息 `thoughts` / reasoning / 原始输出；连续工具调用默认收缩 |
| [attachment-file-icons.md](attachment-file-icons.md) | 附件芯片按扩展名区分常用文档图标（xlsx / pdf / zip 等） |
| [mobile-chat.md](mobile-chat.md) | 移动端：隐藏头像与常显时间/复制；Composer 仅附件+发送；欢迎页底部输入、无 slogan/经验区 |
| [message-list-layout-cache.md](message-list-layout-cache.md) | 消息列表已完成 turn 结构指纹缓存，流式时只重算尾部 |
| [message-list-scroll-follow.md](message-list-scroll-follow.md) | 流式输出贴底跟随：上滑脱离、贴底/按钮恢复 |
| [message-turn-pagination.md](message-turn-pagination.md) | 消息按用户回合分页：滚顶更早、around 后滚底更新、跳转最新 |
| [tool-payload-memory.md](tool-payload-memory.md) | 当前回合的终端输出和工具正文：内存里只留摘要，展开再取 |
| [conversation-nav.md](conversation-nav.md) | 主区右缘短横条导航：全量用户消息，点击 around 定位 |
| [streaming-markdown-throttle.md](streaming-markdown-throttle.md) | 流式 Markdown 渲染节流：默认 100ms，长文 250ms |
| [subagent-stream-ui-perf.md](subagent-stream-ui-perf.md) | 多子 Agent 并发时流式 UI 批处理与 SubAgentFrame 减负 |
| [background-job-ui-consistency.md](background-job-ui-consistency.md) | 后台 job 宿主状态 / 占用对账 / 标题与统计嵌套一致性 |
| [workspace-file-preview-mode.md](workspace-file-preview-mode.md) | 右侧工作区原文 / 预览共用模式（按扩展名注册） |
| [workspace-file-preview-find.md](workspace-file-preview-find.md) | 右侧工作区文本预览查找（⌘/Ctrl+F、上下匹配） |
| [workspace-file-preview-json.md](workspace-file-preview-json.md) | 右侧工作区 JSON 可折叠预览（挂在共用原文 / 预览上） |
| [workspace-file-preview-html.md](workspace-file-preview-html.md) | 右侧工作区 HTML 沙箱预览（挂在共用原文 / 预览上） |
| [workspace-panel-refresh.md](workspace-panel-refresh.md) | 右侧工作区打开 / 切会话 / 切 Tab 时的刷新约定；文件树查找 |
| 轮次修改摘要 | 每轮页脚「修改了 N 个文件」，不在输入框上方。见 [`turn-change-summary.md`](turn-change-summary.md)；基线 Diff 见 [`../developer/turn-file-baseline-review.md`](../developer/turn-file-baseline-review.md) |
| 侧边栏会话搜索 | 第二行展示命中关键词附近的 snippet；点击正文命中结果会定位到对应消息并短暂高亮。详见 [`../internals/sidebar-conversation-search.md`](../internals/sidebar-conversation-search.md) |
| 新会话空状态 | 「热门」与分类并列 Tab（`GET /api/experiences/home`）；搜索关键词（`GET /api/experiences?q=`）；点击后切换绑定智能体并将 `prompt_text` 填入输入框 |
| 经验封面图 | 欢迎页卡片分两栏：标题栏（左标题 + 右 **24×24** 徽章，垂直居中对齐）；内容栏（正文独占整行）；**推荐经验**整块默认收缩，点击标题栏展开；上传最小 **200×200** 像素，建议 1:1 |
| 经验卡片字数 | 标题最多 **15** 字、说明（`narrative_text` / excerpt）最多 **45** 字；与官网创建/编辑表单及 API 校验一致（`experience_card_limits`） |

[返回文档总索引](../README.md)
