//! Embedded zh-CN / en message catalogs, keyed by dotted string keys.
//!
//! Kept as plain `match` arms (no extra dependency like `phf`) since the catalog is small
//! and changes rarely; the compiler turns this into an efficient jump table either way.

pub(super) fn lookup_zh_cn(key: &str) -> Option<&'static str> {
    Some(match key {
        // tools/display.rs — tool display labels & summaries.
        "tool.terminal.label" => "终端命令",
        "tool.terminal.label_elevated" => "终端命令（提权）",
        "tool.file.read" => "读取文件",
        "tool.file.write" => "写入文件",
        "tool.file.edit" => "编辑文件",
        "tool.file.glob" => "搜索文件",
        "tool.file.grep" => "搜索内容",
        "tool.file.list" => "列出目录",
        "tool.file.generic" => "文件操作",
        "tool.mouse.prefix" => "鼠标",
        "tool.mouse.click" => "点击",
        "tool.mouse.double_click" => "双击",
        "tool.mouse.right_click" => "右键",
        "tool.mouse.hover" => "悬停",
        "tool.mouse.drag" => "拖拽",
        "tool.mouse.scroll" => "滚动",
        "tool.mouse.type_text" => "输入文字",
        "tool.input.prefix" => "文本输入",
        "tool.modified_click.prefix" => "修饰点击",
        "tool.hotkey.label" => "快捷键",
        "tool.wait.label" => "等待 {secs} 秒",
        "tool.clipboard.prefix" => "剪贴板",
        "tool.clipboard.read" => "读取",
        "tool.clipboard.write" => "写入",
        "tool.clipboard.op" => "操作",
        "tool.skill.read_resource" => "读取技能资源",
        "tool.skill.load" => "加载技能",
        "tool.skill.update" => "更新技能",
        "tool.skill.generic" => "技能",
        "tool.session_search.label" => "搜索会话",
        "tool.session_read.label" => "读取会话",
        "tool.session_read.offset" => "第 {n} 条",
        "tool.web_search.label" => "联网搜索",
        "tool.web_fetch.label" => "抓取网页",
        "tool.media_understand.label" => "媒体理解",
        "tool.run_subagent.label" => "委派子任务",
        "tool.read_lints.label" => "代码检查",
        "tool.task_board.prefix" => "任务板",
        "tool.task_board.patch" => "更新",
        "tool.task_board.replace" => "替换",
        "tool.task_board.init" => "初始化",
        "tool.task_board.prune" => "清理",
        "tool.task_board.finalize" => "完成",
        "tool.task_board.check_deps" => "检查依赖",
        "tool.task_board.get" => "读取",
        "tool.task_board.generic" => "操作",
        "tool.task_board.rows" => "{label} · {count} 行",
        "tool.list_apps.label" => "列出应用",
        "tool.launch_app.label" => "启动应用",
        "tool.cron_job.create" => "创建定时任务",
        "tool.cron_job.list" => "列出定时任务",
        "tool.cron_job.enable" => "启用定时任务",
        "tool.cron_job.disable" => "停用定时任务",
        "tool.cron_job.delete" => "删除定时任务",
        "tool.cron_job.generic" => "定时任务",
        "tool.job.list" => "查看后台任务",
        "tool.job.status" => "后台任务状态",
        "tool.job.await" => "等待后台任务",
        "tool.job.cancel" => "取消后台任务",
        "tool.job.generic" => "后台任务",
        "tool.ask_user.label" => "询问用户",
        "tool.response.label" => "回复用户",
        "tool.index" => "索引 {idx}",

        // agents/agent_ui.rs — composer labels.
        "agent.ui.supervisor" => "团队模式",
        "agent.ui.general" => "通用助手",
        "agent.ui.coder" => "氛围编程",
        "agent.ui.computer" => "电脑操控",
        "agent.ui.explore" => "代码探索",
        "agent.ui.general_worker" => "通用执行",
        "agent.ui.research" => "深度研究",

        // UiToast emitters.
        "toast.subagent_context_compressing" => "子任务上下文超限，正在压缩后继续",
        "toast.subagent_cancelled_computer" => "计算机操作已取消",
        "toast.subagent_cancelled_generic" => "子 Agent 已停止",
        "toast.context_compressing_retry_later" => "上下文超限，正在压缩（请之后重发）",
        "toast.context_compressing_retry" => "上下文超限，正在压缩后重试",
        "toast.context_too_large" => "上下文过大且无法压缩保留区，请新开对话或删减内容",
        "toast.context_too_large_short" => "上下文过大且无法压缩，请新开对话或删减内容",
        "toast.subagent_context_too_large" => "子任务上下文过大且无法压缩，请新开对话或缩小任务范围",
        "toast.single_agent_context_compressing" => "上下文超限，正在压缩后继续",
        "toast.task_board_trim" => "任务板更新后已将较早 {count} 条对话从上下文排除",
        "toast.memory_updated" => "已更新记忆：{items}",
        "toast.compress.summary_failed_main" => {
            "摘要生成失败，已丢弃较早 {dropped} 条记录，并保留最近 {keep} 轮用户消息"
        }
        "toast.compress.success_main" => "已压缩较早 {dropped} 条对话为摘要，并保留最近对话",
        "toast.compress.summary_failed_sub" => "{name} 子任务：摘要失败，已丢弃较早 {dropped} 条记录",
        "toast.compress.success_sub" => "{name} 子任务：已压缩较早 {dropped} 条记录为摘要",
        "toast.compress.sub_agent_fallback_name" => "子 Agent",

        // Host errors — Tauri commands / server, user-visible.
        "err.tool_call_not_pending" => "未找到待审批的工具调用",
        "err.ask_user_not_pending" => "未找到待选择的 ask_user 请求",
        "err.terminal_input_not_pending" => "未找到待输入的终端请求",
        "err.api_key_missing" => "尚未配置 API Key",
        "err.media_file_missing" => "媒体文件不存在: {path}",
        "err.save_path_empty" => "保存路径为空",
        "err.save_path_invalid" => "保存路径无效",
        "err.file_content_empty" => "文件内容为空",
        "err.file_manager_unavailable" => "未找到可用的文件管理器",
        "err.platform_unsupported" => "当前平台不支持",
        "err.file_missing" => "文件不存在: {path}",
        "err.oss_not_configured" => "OSS 未配置，请登录 Pointer 账户或联系管理员在官网配置 OSS",
        "err.video_oss_need_platform" => {
            "视频上传需要平台 OSS 配置，请登录 Pointer 账户或联系管理员在官网配置 OSS"
        }
        "err.video_must_use_oss" => "视频请通过 OSS 上传：使用文件选择后自动上传，勿直接读取整文件到内存",
        "err.file_too_large" => "文件超过 {limit} MB 上限",
        "err.dir_name_invalid" => "目录名称无效",
        "err.parent_dir_unavailable" => "父目录不存在或不可访问",
        "err.plugin_not_found" => "插件不存在: {plugin_id}",
        "err.create_dir_failed" => "创建目录失败: {e}",
        "err.write_failed" => "写入失败: {e}",
        "err.open_finder_failed" => "打开 Finder 失败: {e}",
        "err.open_file_manager_failed" => "打开文件管理器失败: {e}",
        "err.open_file_failed" => "打开文件失败: {e}",
        "err.read_file_meta_failed" => "读取文件信息失败: {e}",
        "err.read_file_failed" => "读取文件失败: {e}",
        "err.no_desktop_screenshot" => {
            "暂无桌面截图：请先完成一次截图处理（发送 Computer 消息），或确认会话 ID 正确。"
        }
        "err.oss_bucket_missing" => {
            "OSS Bucket 不存在，请在阿里云创建对应 Bucket 或将 Endpoint 配置为「https://<bucket>.oss-<region>.aliyuncs.com」格式。详情：{msg}"
        }
        "err.oss_credentials_invalid" => {
            "OSS 凭据无效，请检查官网 OSS 配置中的 AccessKey。详情：{msg}"
        }
        "err.video_upload_timeout" => "视频上传超时，请检查网络后重试。详情：{msg}",
        "err.login_required" => "请先登录 Pointer 账户",
        "err.login_required_platform" => "请先登录 Pointer 平台账户",
        "err.cloud_console_url_invalid" => "控制台地址无效",
        "err.cloud_window_not_open" => "云主机窗口未打开",
        "err.cloud_agent_not_ready" => "智能体尚未就绪，无法打开",
        "err.cloud_window_create_failed" => "创建云主机窗口失败: {e}",
        "err.channels_load_failed" => "加载通道配置失败: {e}",
        "err.channels_save_failed" => "保存通道配置失败: {e}",
        "err.weixin_login_start_failed" => "微信扫码登录启动失败: {e}",
        "err.weixin_credentials_read_failed" => "读取微信凭证失败: {e}",
        "err.pairing_approve_failed" => "配对审批失败: {e}",
        "err.pairing_code_invalid" => {
            "配对码无效或已过期（请确认点击了正确通道的批准按钮，或使用最新收到的配对码）"
        }
        "err.channel_registration_start_failed" => "{channel} 扫码注册启动失败: {e}",
        "err.open_system_settings_failed" => "无法打开系统设置: {e}",
        "err.app_bundle_path_missing" => "无法定位 Pointer 应用包路径",

        // Misc UI strings owned by the Rust host.
        "toast.capture_purged" => "截图过期已清理",
        "ui.cloud_host" => "云主机",
        "tray.show_pointer" => "显示 Pointer",
        "tray.quit" => "退出",
        "macos.perm.screen_recording" => "屏幕录制",
        "macos.perm.accessibility" => "辅助功能",
        "macos.perm.drag_screen" => "拖到右侧「屏幕录制」列表",
        "macos.perm.drag_accessibility" => "拖到右侧「辅助功能」列表",
        "macos.perm.drag_hint" => "拖入后保持启用，将自动进入下一步",
        "popup.wecom_auth_title" => "企业微信授权",

        _ => return None,
    })
}

pub(super) fn lookup_en(key: &str) -> Option<&'static str> {
    Some(match key {
        "tool.terminal.label" => "Terminal command",
        "tool.terminal.label_elevated" => "Terminal command (elevated)",
        "tool.file.read" => "Read file",
        "tool.file.write" => "Write file",
        "tool.file.edit" => "Edit file",
        "tool.file.glob" => "Search files",
        "tool.file.grep" => "Search content",
        "tool.file.list" => "List directory",
        "tool.file.generic" => "File operation",
        "tool.mouse.prefix" => "Mouse",
        "tool.mouse.click" => "Click",
        "tool.mouse.double_click" => "Double click",
        "tool.mouse.right_click" => "Right click",
        "tool.mouse.hover" => "Hover",
        "tool.mouse.drag" => "Drag",
        "tool.mouse.scroll" => "Scroll",
        "tool.mouse.type_text" => "Type text",
        "tool.input.prefix" => "Text input",
        "tool.modified_click.prefix" => "Modifier click",
        "tool.hotkey.label" => "Hotkey",
        "tool.wait.label" => "Wait {secs}s",
        "tool.clipboard.prefix" => "Clipboard",
        "tool.clipboard.read" => "Read",
        "tool.clipboard.write" => "Write",
        "tool.clipboard.op" => "Operation",
        "tool.skill.read_resource" => "Read skill resource",
        "tool.skill.load" => "Load skill",
        "tool.skill.update" => "Update skill",
        "tool.skill.generic" => "Skill",
        "tool.session_search.label" => "Search sessions",
        "tool.session_read.label" => "Read session",
        "tool.session_read.offset" => "message #{n}",
        "tool.web_search.label" => "Web search",
        "tool.web_fetch.label" => "Fetch webpage",
        "tool.media_understand.label" => "Media understanding",
        "tool.run_subagent.label" => "Delegate subtask",
        "tool.read_lints.label" => "Lint check",
        "tool.task_board.prefix" => "Task board",
        "tool.task_board.patch" => "Update",
        "tool.task_board.replace" => "Replace",
        "tool.task_board.init" => "Initialize",
        "tool.task_board.prune" => "Prune",
        "tool.task_board.finalize" => "Finalize",
        "tool.task_board.check_deps" => "Check dependencies",
        "tool.task_board.get" => "Read",
        "tool.task_board.generic" => "Operation",
        "tool.task_board.rows" => "{label} · {count} rows",
        "tool.list_apps.label" => "List apps",
        "tool.launch_app.label" => "Launch app",
        "tool.cron_job.create" => "Create scheduled task",
        "tool.cron_job.list" => "List scheduled tasks",
        "tool.cron_job.enable" => "Enable scheduled task",
        "tool.cron_job.disable" => "Disable scheduled task",
        "tool.cron_job.delete" => "Delete scheduled task",
        "tool.cron_job.generic" => "Scheduled task",
        "tool.job.list" => "View background tasks",
        "tool.job.status" => "Background task status",
        "tool.job.await" => "Await background task",
        "tool.job.cancel" => "Cancel background task",
        "tool.job.generic" => "Background task",
        "tool.ask_user.label" => "Ask user",
        "tool.response.label" => "Reply to user",
        "tool.index" => "Index {idx}",

        "agent.ui.supervisor" => "Team mode",
        "agent.ui.general" => "General assistant",
        "agent.ui.coder" => "Vibe coding",
        "agent.ui.computer" => "Computer control",
        "agent.ui.explore" => "Code explore",
        "agent.ui.general_worker" => "General worker",
        "agent.ui.research" => "Deep research",

        "toast.subagent_context_compressing" => {
            "Subtask context exceeded, compressing before continuing"
        }
        "toast.subagent_cancelled_computer" => "Computer operation cancelled",
        "toast.subagent_cancelled_generic" => "Sub agent stopped",
        "toast.context_compressing_retry_later" => {
            "Context exceeded, compressing (please resend later)"
        }
        "toast.context_compressing_retry" => "Context exceeded, compressing then retrying",
        "toast.context_too_large" => {
            "Context too large to compress within the retained region; start a new conversation or trim content"
        }
        "toast.context_too_large_short" => {
            "Context too large to compress; start a new conversation or trim content"
        }
        "toast.subagent_context_too_large" => {
            "Subtask context too large to compress; start a new conversation or narrow the task"
        }
        "toast.single_agent_context_compressing" => {
            "Context exceeded, compressing before continuing"
        }
        "toast.task_board_trim" => "Task board update excluded {count} earlier messages from context",
        "toast.memory_updated" => "Memory updated: {items}",
        "toast.compress.summary_failed_main" => {
            "Summary generation failed; dropped {dropped} earlier messages and kept the most recent {keep} user turns"
        }
        "toast.compress.success_main" => {
            "Compressed {dropped} earlier messages into a summary and kept recent conversation"
        }
        "toast.compress.summary_failed_sub" => {
            "{name} subtask: summary failed, dropped {dropped} earlier messages"
        }
        "toast.compress.success_sub" => {
            "{name} subtask: compressed {dropped} earlier messages into a summary"
        }
        "toast.compress.sub_agent_fallback_name" => "Sub agent",

        "err.tool_call_not_pending" => "No pending tool call awaiting approval",
        "err.ask_user_not_pending" => "No pending ask_user request awaiting a choice",
        "err.terminal_input_not_pending" => "No pending terminal request awaiting input",
        "err.api_key_missing" => "API key is not configured yet",
        "err.media_file_missing" => "Media file does not exist: {path}",
        "err.save_path_empty" => "Save path is empty",
        "err.save_path_invalid" => "Save path is invalid",
        "err.file_content_empty" => "File content is empty",
        "err.file_manager_unavailable" => "No available file manager found",
        "err.platform_unsupported" => "Current platform is not supported",
        "err.file_missing" => "File does not exist: {path}",
        "err.oss_not_configured" => {
            "OSS is not configured; sign in to your Pointer account or ask an admin to configure OSS on the website"
        }
        "err.video_oss_need_platform" => {
            "Video upload requires platform OSS configuration; sign in to your Pointer account or ask an admin to configure OSS on the website"
        }
        "err.video_must_use_oss" => {
            "Please upload videos via OSS: selecting a file uploads it automatically; do not read the whole file into memory"
        }
        "err.file_too_large" => "File exceeds the {limit} MB limit",
        "err.dir_name_invalid" => "Directory name is invalid",
        "err.parent_dir_unavailable" => "Parent directory does not exist or is not accessible",
        "err.plugin_not_found" => "Plugin not found: {plugin_id}",
        "err.create_dir_failed" => "Failed to create directory: {e}",
        "err.write_failed" => "Write failed: {e}",
        "err.open_finder_failed" => "Failed to open Finder: {e}",
        "err.open_file_manager_failed" => "Failed to open file manager: {e}",
        "err.open_file_failed" => "Failed to open file: {e}",
        "err.read_file_meta_failed" => "Failed to read file info: {e}",
        "err.read_file_failed" => "Failed to read file: {e}",
        "err.no_desktop_screenshot" => {
            "No desktop screenshot yet: complete a screenshot step first (send a Computer message), or confirm the conversation ID is correct."
        }
        "err.oss_bucket_missing" => {
            "OSS Bucket does not exist; create the Bucket in Alibaba Cloud or set Endpoint to https://<bucket>.oss-<region>.aliyuncs.com. Details: {msg}"
        }
        "err.oss_credentials_invalid" => {
            "Invalid OSS credentials; check the AccessKey in the website OSS settings. Details: {msg}"
        }
        "err.video_upload_timeout" => {
            "Video upload timed out; check your network and try again. Details: {msg}"
        }
        "err.login_required" => "Please sign in to your Pointer account",
        "err.login_required_platform" => "Please sign in to your Pointer platform account",
        "err.cloud_console_url_invalid" => "Console URL is invalid",
        "err.cloud_window_not_open" => "Cloud host window is not open",
        "err.cloud_agent_not_ready" => "Agent is not ready and cannot be opened",
        "err.cloud_window_create_failed" => "Failed to create cloud host window: {e}",
        "err.channels_load_failed" => "Failed to load channel config: {e}",
        "err.channels_save_failed" => "Failed to save channel config: {e}",
        "err.weixin_login_start_failed" => "Failed to start Weixin QR login: {e}",
        "err.weixin_credentials_read_failed" => "Failed to read Weixin credentials: {e}",
        "err.pairing_approve_failed" => "Pairing approval failed: {e}",
        "err.pairing_code_invalid" => {
            "Pairing code is invalid or expired (confirm you approved the correct channel, or use the latest code)"
        }
        "err.channel_registration_start_failed" => {
            "Failed to start {channel} QR registration: {e}"
        }
        "err.open_system_settings_failed" => "Failed to open System Settings: {e}",
        "err.app_bundle_path_missing" => "Could not locate the Pointer app bundle path",

        "toast.capture_purged" => "Expired screenshots cleaned up",
        "ui.cloud_host" => "Cloud host",
        "tray.show_pointer" => "Show Pointer",
        "tray.quit" => "Quit",
        "macos.perm.screen_recording" => "Screen Recording",
        "macos.perm.accessibility" => "Accessibility",
        "macos.perm.drag_screen" => "Drag to the Screen Recording list on the right",
        "macos.perm.drag_accessibility" => "Drag to the Accessibility list on the right",
        "macos.perm.drag_hint" => "Keep it enabled after dropping; the next step will start automatically",
        "popup.wecom_auth_title" => "WeCom authorization",

        _ => return None,
    })
}
