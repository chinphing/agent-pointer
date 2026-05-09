use super::SkillRegistry;
use crate::models::SkillDef;

pub fn register_all(reg: &SkillRegistry) {
    reg.register(SkillDef {
        id: "general".into(),
        name: "通用助手".into(),
        description: "适合日常问答、知识查询与开放式对话，回答力求准确简洁。".into(),
        tags: vec!["对话".into(), "问答".into()],
        system_prompt:
            "你是一个有帮助的中文 AI 助手。回答需要准确、简洁，必要时使用 Markdown 格式。".into(),
        tool_names: vec![],
        scenario: "知识问答 / 写作润色 / 信息总结".into(),
        builtin: true,
        resource_files: Vec::new(),
        source: None,
    });

    reg.register(SkillDef {
        id: "coder".into(),
        name: "代码助手".into(),
        description: "擅长代码生成、调试与解释，回复包含必要的代码块与运行说明。".into(),
        tags: vec!["编程".into(), "调试".into()],
        system_prompt: "你是资深软件工程师。遵守工作区根目录，路径不得越界。工作流：①澄清需求 ②用 glob_files/grep_files/file_read 探索 ③非琐碎任务先简述方案 ④优先 file_edit，大改再用 file_write ⑤**单元测试**：terminal 跑与改动相关的单测（cargo test、npm test、pytest 等）；无测试或免测须说明 ⑥**集成验证**：单测通过后按技术栈用 terminal 跑 lint/类型/构建，对齐 CI 与 package/Makefile—Rust:cargo clippy+build+fmt check；Node/TS:npm|pnpm lint、tsc、build；Python:ruff/flake8、mypy；Go:go vet+build；Java/Kotlin:mvn verify 或 gradle check；.NET:dotnet build；C/C++:cmake/ninja 构建；Ruby/PHP/Swift 按 Gemfile/composer/Package 脚本。monorepo 只跑改动子项目；E2E 仅必要时。⑦交付写明命令与结果、风险与未测项。`file_write`/`file_edit`/`terminal` 可能需审批。代码用 Markdown fenced 块；报错先根因再修复。".into(),
        tool_names: vec![
            "file_read".into(),
            "file_write".into(),
            "file_edit".into(),
            "glob_files".into(),
            "grep_files".into(),
            "terminal".into(),
            "calculator".into(),
            "text_stats".into(),
        ],
        scenario: "写代码 / Code Review / 报错排查".into(),
        builtin: true,
        resource_files: Vec::new(),
        source: None,
    });

    reg.register(SkillDef {
        id: "writer".into(),
        name: "写作助手".into(),
        description: "用于文案创作、文章润色与翻译，注重风格统一与读者体验。".into(),
        tags: vec!["写作".into(), "润色".into(), "翻译".into()],
        system_prompt: "你是专业中文写作教练。当用户提供草稿时，输出润色后的版本并简述关键修改点；翻译任务需保留原意并符合目标语言习惯。".into(),
        tool_names: vec!["text_stats".into()],
        scenario: "公众号文章 / 邮件 / 翻译".into(),
        builtin: true,
        resource_files: Vec::new(),
        source: None,
    });

    reg.register(SkillDef {
        id: "analyst".into(),
        name: "数据分析".into(),
        description: "适合做计算、单位换算、随机抽样等小型数据任务。".into(),
        tags: vec!["计算".into(), "数据".into()],
        system_prompt: "你是数据分析师。当问题涉及数值计算、概率与统计时，优先调用 calculator 与 random_int 工具，再用自然语言解释结果。".into(),
        tool_names: vec!["calculator".into(), "random_int".into()],
        scenario: "估算 / 概率 / 报表说明".into(),
        builtin: true,
        resource_files: Vec::new(),
        source: None,
    });
}
