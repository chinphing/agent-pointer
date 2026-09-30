//! Embedded zh-CN / en catalogs for user-visible Rust UI strings.

use super::UiLocale;
use std::collections::HashMap;
use std::sync::OnceLock;

macro_rules! entries {
    ($($key:literal => $zh:literal, $en:literal),* $(,)?) => {
        #[cfg(test)] // parity test in `i18n::mod` is the only consumer
        pub const ALL_KEYS: &[&str] = &[$($key),*];
        fn zh_map() -> HashMap<&'static str, &'static str> {
            HashMap::from([$(($key, $zh)),*])
        }
        fn en_map() -> HashMap<&'static str, &'static str> {
            HashMap::from([$(($key, $en)),*])
        }
    };
}

entries! {
    // agents
    "agents.general" => "通用助手", "General",
    "agents.coder" => "氛围编程", "Vibe coding",
    "agents.computer" => "电脑操控", "Computer use",
    "agents.explore" => "代码探索", "Code explore",
    "agents.generalWorker" => "通用执行", "General worker",
    "agents.analyst" => "深度研究", "Deep research",
    // tools
    "tools.fileRead" => "读取文件", "Read file",
    "tools.fileWrite" => "写入文件", "Write file",
    "tools.fileEdit" => "编辑文件", "Edit file",
    "tools.fileGlob" => "搜索文件", "Find files",
    "tools.fileGrep" => "搜索内容", "Search content",
    "tools.fileList" => "列出目录", "List directory",
    "tools.fileOp" => "文件操作", "File operation",
    "tools.click" => "点击", "Click",
    "tools.doubleClick" => "双击", "Double-click",
    "tools.rightClick" => "右键", "Right-click",
    "tools.hover" => "悬停", "Hover",
    "tools.drag" => "拖拽", "Drag",
    "tools.scroll" => "滚动", "Scroll",
    "tools.typeText" => "输入文字", "Type text",
    "tools.hotkey" => "快捷键", "Hotkey",
    "tools.waitSecs" => "等待 {secs} 秒", "Wait {secs}s",
    "tools.mouse" => "鼠标 · {action}", "Mouse · {action}",
    "tools.textInput" => "文本输入 · {action}", "Type · {action}",
    "tools.modClick" => "修饰点击 · {action}", "Mod-click · {action}",
    "tools.clipboard" => "剪贴板 · {action}", "Clipboard · {action}",
    "tools.read" => "读取", "Read",
    "tools.write" => "写入", "Write",
    "tools.op" => "操作", "Action",
    "tools.cronCreate" => "创建定时任务", "Create schedule",
    "tools.cronList" => "列出定时任务", "List schedules",
    "tools.cronEnable" => "启用定时任务", "Enable schedule",
    "tools.cronDisable" => "停用定时任务", "Disable schedule",
    "tools.cronDelete" => "删除定时任务", "Delete schedule",
    "tools.cron" => "定时任务", "Schedule",
    "tools.indexN" => "索引 {idx}", "Index {idx}",
    "tools.patch" => "更新", "Update",
    "tools.replace" => "替换", "Replace",
    "tools.init" => "初始化", "Initialize",
    "tools.prune" => "清理", "Prune",
    "tools.finalize" => "完成", "Finalize",
    "tools.checkDeps" => "检查依赖", "Check deps",
    "tools.get" => "读取", "Read",
    "tools.terminal" => "终端命令", "Terminal",
    "tools.terminalElevated" => "终端命令（提权）", "Terminal (elevated)",
    "tools.taskBoard" => "任务板 · {action}", "Task board · {action}",
    "tools.skillLoad" => "加载技能", "Load skill",
    "tools.skillReadResource" => "读取技能资源", "Read skill resource",
    "tools.skillPatch" => "更新技能", "Update skill",
    "tools.skill" => "技能", "Skill",
    "tools.sessionSearch" => "搜索会话", "Search chats",
    "tools.sessionRead" => "读取会话", "Read chat",
    "tools.webSearch" => "联网搜索", "Web search",
    "tools.webFetch" => "抓取网页", "Fetch page",
    "tools.mediaUnderstand" => "媒体理解", "Media understanding",
    "tools.delegate" => "委派子任务", "Delegate subtask",
    "tools.runSubagent" => "委派子任务", "Delegate subtask",
    "tools.readLints" => "代码检查", "Lint check",
    "tools.listApps" => "列出应用", "List apps",
    "tools.launchApp" => "启动应用", "Launch app",
    "tools.jobList" => "查看后台任务", "List background jobs",
    "tools.jobStatus" => "后台任务状态", "Background job status",
    "tools.jobAwait" => "等待后台任务", "Await background job",
    "tools.jobCancel" => "取消后台任务", "Cancel background job",
    "tools.job" => "后台任务", "Background job",
    "tools.askUser" => "询问用户", "Ask user",
    "tools.response" => "回复用户", "Reply to user",
    "tools.nthItem" => "第 {n} 条", "Item {n}",
    "tools.rows" => "{count} 行", "{count} rows",
    "tools.checkDepsItem" => "{method} · #{id}", "{method} · #{id}",
    "tools.methodRows" => "{method} · {count} 行", "{method} · {count} rows",
    "tools.messageN" => "第 {n} 条", "Message {n}",
    // toasts
    "toast.contextOverflowCompressContinue" => "上下文超限，正在压缩后继续", "Context limit reached; compressing and continuing",
    "toast.contextOverflowCompressRetry" => "上下文超限，正在压缩后重试", "Context limit reached; compressing and retrying",
    "toast.contextOverflowCompressLater" => "上下文超限，正在压缩（请之后重发）", "Context limit reached; compressing (resend afterward)",
    "toast.contextOverflowUnrecoverable" => "上下文过大且无法压缩保留区，请新开对话或删减内容", "Context too large to compress. Start a new chat or trim content",
    "toast.subContextOverflowCompressContinue" => "子任务上下文超限，正在压缩后继续", "Subtask context limit reached; compressing and continuing",
    "toast.compressionDone" => "已压缩较早 {dropped} 条对话为摘要，并保留最近对话", "Compressed earlier {dropped} messages into a summary and kept recent chat",
    "toast.compressionSummaryFailed" => "摘要生成失败，已丢弃较早 {dropped} 条记录，并保留最近 {keep} 轮用户消息", "Summary failed; dropped earlier {dropped} records and kept the last {keep} user turns",
    "toast.subCompressionDone" => "{name} 子任务：已压缩较早 {dropped} 条记录为摘要", "{name} subtask: compressed earlier {dropped} records into a summary",
    "toast.subCompressionSummaryFailed" => "{name} 子任务：摘要失败，已丢弃较早 {dropped} 条记录", "{name} subtask: summary failed; dropped earlier {dropped} records",
    "toast.subAgentFallbackName" => "子 Agent", "Sub-agent",
    "toast.computerCancelled" => "计算机操作已取消", "Computer use cancelled",
    "toast.subAgentStopped" => "子 Agent 已停止", "Sub-agent stopped",
    "toast.memoryUpdated" => "已更新记忆：{items}", "Memory updated: {items}",
    "toast.taskBoardTrimmed" => "任务板更新后已将较早 {count} 条对话从上下文排除", "After task board update, excluded earlier {count} messages from context",
    // errors
    "errors.generationStopped" => "已停止生成", "Generation stopped",
    "errors.leadToolBudget" => "单智能体模式下工具调用轮次已达上限（{max}）。请新开对话或在设置中调高上限。", "Tool-call rounds in single-agent mode reached the limit ({max}). Start a new chat or raise the limit in Settings.",
    "errors.subAgentToolBudget" => "子 Agent 工具调用轮次已达上限（{max}）。请新开对话或在设置中调高上限。", "Sub-agent tool-call rounds reached the limit ({max}). Start a new chat or raise the limit in Settings.",
    "errors.subAgentInnerToolBudget" => "子 Agent 内工具调用轮次已达上限（{max}）。请新开对话或在设置中调高上限。", "Tool-call rounds inside the sub-agent reached the limit ({max}). Start a new chat or raise the limit in Settings.",
}

fn catalog(locale: UiLocale) -> &'static HashMap<&'static str, &'static str> {
    static ZH: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    static EN: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    match locale {
        UiLocale::ZhCn => ZH.get_or_init(zh_map),
        UiLocale::En => EN.get_or_init(en_map),
    }
}

pub fn lookup(locale: UiLocale, key: &str) -> Option<&'static str> {
    catalog(locale).get(key).copied()
}
