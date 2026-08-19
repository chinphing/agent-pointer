//! 插件启用装配：把插件的 skills / agents / rules / tools 接入现有 Registry。
//!
//! 设计稿 §4.4 能力单元 → 现有代码映射。`activate` 在插件授权启用后调用；
//! `deactivate` 在禁用 / 卸载时调用（按 `plugin_id` 精确注销，幂等）。

use crate::agents::AgentRegistry;
use crate::dispatcher::HookRegistry;
use crate::extensions::{
    new_extension_message_id, now_ms, ExtensionRegistry, MessageLoopPromptsAfterContext,
    MessageLoopPromptsAfterHook,
};
use crate::models::{ChatMessage, Role};
use crate::plugins::registry::PluginRecord;
use crate::plugins::tool_provider::build_sidecar_tool_entry;
use crate::skills::{external::load_skill_from_dir, SkillRegistry};
use crate::tools::ToolRegistry;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 插件规则注入 hook 的 override_key 前缀（每插件一个，可被重复注册替换）。
fn plugin_rules_hook_key(plugin_id: &str) -> String {
    format!("plugin:{plugin_id}:rules")
}

/// 激活一个插件：注册其声明的工具 / skill / agent / rule / hooks 到现有 Registry。
/// 幂等：先按 plugin_id 注销再注册，重复调用安全。
pub fn activate_plugin(
    tools: &ToolRegistry,
    skills: &SkillRegistry,
    agents: &AgentRegistry,
    extensions: &ExtensionRegistry,
    hook_registry: &HookRegistry,
    record: &PluginRecord,
) -> Result<()> {
    deactivate_plugin(tools, skills, agents, extensions, hook_registry, &record.id);
    activate_tools(tools, record)?;
    activate_skills(skills, record)?;
    activate_agents(agents, record)?;
    activate_rules(extensions, record)?;
    crate::plugins::hooks::register_plugin_hooks(hook_registry, record)?;
    log::info!(
        "plugin activated: {} (dir={})",
        record.id,
        record.dir.display()
    );
    Ok(())
}

/// 注销一个插件：按 plugin_id 移除全部已注册能力（含 hooks）。
pub fn deactivate_plugin(
    tools: &ToolRegistry,
    skills: &SkillRegistry,
    agents: &AgentRegistry,
    extensions: &ExtensionRegistry,
    hook_registry: &HookRegistry,
    plugin_id: &str,
) {
    let n_tools = tools.unregister_by_plugin(plugin_id);
    let n_skills = skills.unregister_by_plugin(plugin_id);
    let n_agents = agents.unregister_by_plugin(plugin_id);
    let removed_rule =
        extensions.remove_message_loop_prompts_after(&plugin_rules_hook_key(plugin_id));
    crate::plugins::hooks::unregister_plugin_hooks(hook_registry, plugin_id);
    log::info!(
        "plugin deactivated: {} (tools={n_tools} skills={n_skills} agents={n_agents} rules={removed_rule})",
        plugin_id
    );
}

/// 注册 `[[tools.tool]]` 声明的 sidecar 工具。
fn activate_tools(tools: &ToolRegistry, record: &PluginRecord) -> Result<()> {
    for decl in &record.manifest.tools.tool {
        let entry = build_sidecar_tool_entry(&record.id, &record.dir, decl)
            .with_context(|| format!("插件工具注册失败: {}", decl.name))?;
        tools.register(entry);
    }
    Ok(())
}

/// 注册 `[skills] path` 目录下的每个 skill 子目录（含 SKILL.md）。
fn activate_skills(skills: &SkillRegistry, record: &PluginRecord) -> Result<()> {
    let Some(dir_ref) = &record.manifest.skills else {
        return Ok(());
    };
    let root = resolve_plugin_relative(&record.dir, &dir_ref.path)?;
    if !root.exists() {
        log::warn!(
            "plugin {}: skills 目录不存在: {}",
            record.id,
            root.display()
        );
        return Ok(());
    }
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match load_skill_from_dir(&path) {
            Ok(mut def) => {
                def.plugin_id = Some(record.id.clone());
                def.provenance = "external".to_string();
                skills.register(def);
            }
            Err(err) => {
                log::warn!(
                    "plugin {}: skill 加载失败 {}: {err:#}",
                    record.id,
                    path.display()
                );
            }
        }
    }
    Ok(())
}

/// 注册 `[agents] path` 目录下的每个 agent 子目录（含 AGENT.md）。
fn activate_agents(agents: &AgentRegistry, record: &PluginRecord) -> Result<()> {
    let Some(dir_ref) = &record.manifest.agents else {
        return Ok(());
    };
    let root = resolve_plugin_relative(&record.dir, &dir_ref.path)?;
    if !root.exists() {
        log::warn!(
            "plugin {}: agents 目录不存在: {}",
            record.id,
            root.display()
        );
        return Ok(());
    }
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match crate::agents::load_agent_from_dir(&path) {
            Ok(mut agent) => {
                agent.def.plugin_id = Some(record.id.clone());
                agents.register(agent);
            }
            Err(err) => {
                log::warn!(
                    "plugin {}: agent 加载失败 {}: {err:#}",
                    record.id,
                    path.display()
                );
            }
        }
    }
    Ok(())
}

/// 注册 `[rules] path` 目录下的规则文件（.md / .mdc）为 MessageLoopPromptsAfter hook。
fn activate_rules(extensions: &ExtensionRegistry, record: &PluginRecord) -> Result<()> {
    let Some(dir_ref) = &record.manifest.rules else {
        return Ok(());
    };
    let root = resolve_plugin_relative(&record.dir, &dir_ref.path)?;
    if !root.exists() {
        log::warn!("plugin {}: rules 目录不存在: {}", record.id, root.display());
        return Ok(());
    }
    let mut contents = Vec::new();
    collect_rule_files(&root, &mut contents)?;
    if contents.is_empty() {
        return Ok(());
    }
    let hook = PluginRulesHook {
        plugin_id: record.id.clone(),
        plugin_name: record.manifest.plugin.name.clone(),
        rules: contents,
    };
    extensions.register_message_loop_prompts_after(Arc::new(hook));
    Ok(())
}

fn resolve_plugin_relative(plugin_dir: &Path, rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    Ok(plugin_dir.join(p))
}

fn collect_rule_files(root: &Path, out: &mut Vec<RuleFile>) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rule_files(&path, out)?;
        } else {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".md") || lower.ends_with(".mdc")) {
                continue;
            }
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("读取规则文件失败: {}", path.display()))?;
            out.push(RuleFile { name, content });
        }
    }
    Ok(())
}

struct RuleFile {
    name: String,
    content: String,
}

/// 插件规则注入 hook：把插件规则文本追加为每轮最后的 user 消息。
struct PluginRulesHook {
    plugin_id: String,
    plugin_name: String,
    rules: Vec<RuleFile>,
}

#[async_trait]
impl MessageLoopPromptsAfterHook for PluginRulesHook {
    fn override_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(plugin_rules_hook_key(&self.plugin_id))
    }

    fn sort_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(format!("_70_plugin_rules:{}", self.plugin_id))
    }

    async fn execute(&self, ctx: &mut MessageLoopPromptsAfterContext<'_>) -> Result<()> {
        if self.rules.is_empty() {
            return Ok(());
        }
        let body = self
            .rules
            .iter()
            .map(|r| format!("### {}\n{}", r.name, r.content.trim()))
            .collect::<Vec<_>>()
            .join("\n\n");
        let content = format!(
            "【插件规则：{}（{}）】\n{}",
            self.plugin_name, self.plugin_id, body
        );
        ctx.injected_tail.push(ChatMessage {
            id: new_extension_message_id("plugin_rules"),
            role: Role::User,
            content,
            status: "done".into(),
            created_at: now_ms(),
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        });
        Ok(())
    }
}

/// 便捷函数：根据 manifest 构建示例插件目录结构（测试用）。
#[cfg(test)]
pub(crate) fn write_example_plugin(root: &Path, plugin_id: &str) -> Result<PathBuf> {
    use std::fs;
    let dir = root.join(plugin_id);
    fs::create_dir_all(dir.join("skills/demo-skill"))?;
    fs::create_dir_all(dir.join("agents/demo-agent"))?;
    fs::create_dir_all(dir.join("rules"))?;
    fs::create_dir_all(dir.join("bin"))?;

    let manifest = format!(
        r#"[plugin]
id = "{plugin_id}"
name = "示例插件"
version = "1.0.0"
api_version = "v1"
description = "P1 验收示例插件"

[skills]
path = "skills/"

[agents]
path = "agents/"

[rules]
path = "rules/"

[[tools.tool]]
name = "demo_hello"
risk_level = "low"
requires_approval = false
description = "问候演示工具"
exec = {{ command = "bin/demo-tool", transport = "sidecar" }}
"#
    );
    fs::write(dir.join("pointer-plugin.toml"), manifest)?;

    fs::write(
        dir.join("skills/demo-skill/SKILL.md"),
        "---\nname: demo-skill\ndescription: 演示技能\n---\n\n演示内容。\n",
    )?;
    fs::write(
        dir.join("agents/demo-agent/AGENT.md"),
        "---\nid: demo-agent\nname: Demo Agent\ndescription: 演示子代理\nrole: worker\nenabled: true\n---\n\nYou are a demo agent.\n",
    )?;
    fs::write(dir.join("rules/guardrail.md"), "# 规则\n\n插件规则正文。\n")?;
    let tool_path = dir.join("bin/demo-tool");
    fs::write(
        &tool_path,
        "#!/bin/sh\nprintf '{\"ok\": true, \"result\": \"hello\"}\\n'\n",
    )?;
    // 赋予可执行权限（Unix）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tool_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tool_path, perms)?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::ComputerState;
    use crate::agents::AgentProfile;
    use crate::extensions::{MessageLoopPromptsAfterContext, MessageLoopPromptsAfterHook};
    use crate::models::ChatMessage;
    use crate::plugins::registry::{PluginRegistry, PluginStatus};

    fn build_registries() -> (
        ToolRegistry,
        SkillRegistry,
        AgentRegistry,
        ExtensionRegistry,
        crate::dispatcher::HookRegistry,
    ) {
        (
            ToolRegistry::new(),
            SkillRegistry::new(),
            AgentRegistry::new(),
            ExtensionRegistry::new(),
            crate::dispatcher::HookRegistry::new(),
        )
    }

    #[test]
    fn activate_plugin_registers_all_capabilities_with_plugin_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugins");
        let plugin_dir = write_example_plugin(&root, "com.example.demo").unwrap();
        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);

        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        reg.authorize("com.example.demo").unwrap();
        reg.enable("com.example.demo").unwrap();
        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        let record = reg.get("com.example.demo").unwrap();
        assert_eq!(record.status, PluginStatus::Enabled);

        let (tools, skills, agents, extensions, hook_registry) = build_registries();
        activate_plugin(&tools, &skills, &agents, &extensions, &hook_registry, &record).unwrap();

        // 工具：demo_hello 注册且带 plugin_id
        let def = tools.get_def("demo_hello").expect("tool registered");
        assert_eq!(def.name, "demo_hello");
        assert_eq!(tools.tool_risk_level("demo_hello").as_deref(), Some("low"));
        assert!(tools
            .tool_names_by_plugin("com.example.demo")
            .contains(&"demo_hello".to_string()));
        // 工具可执行：sidecar 进程 + JSON 协议
        let out = tools.invoke("demo_hello", serde_json::json!({})).unwrap();
        assert_eq!(out, "hello");

        // skill：demo-skill 注册且带 plugin_id
        let skill = skills.get("demo-skill").expect("skill registered");
        assert_eq!(skill.plugin_id.as_deref(), Some("com.example.demo"));
        assert_eq!(skill.provenance, "external");

        // agent：demo-agent 注册且带 plugin_id
        let agent = agents.get("demo-agent").expect("agent registered");
        let def = agent.def();
        assert_eq!(def.id, "demo-agent");
        assert_eq!(def.plugin_id.as_deref(), Some("com.example.demo"));
        assert_eq!(def.role, "worker");

        // rule：MessageLoopPromptsAfter hook 注册
        let key = plugin_rules_hook_key("com.example.demo");
        let removed = extensions.remove_message_loop_prompts_after(&key);
        assert!(removed, "rules hook should be registered");
        // 再次移除应为 false（幂等）
        assert!(!extensions.remove_message_loop_prompts_after(&key));
    }

    #[test]
    fn deactivate_plugin_removes_all_capabilities() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugins");
        let plugin_dir = write_example_plugin(&root, "com.example.demo").unwrap();
        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);

        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        reg.authorize("com.example.demo").unwrap();
        reg.enable("com.example.demo").unwrap();
        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        let record = reg.get("com.example.demo").unwrap();

        let (tools, skills, agents, extensions, hook_registry) = build_registries();
        activate_plugin(&tools, &skills, &agents, &extensions, &hook_registry, &record).unwrap();
        assert!(tools.get_def("demo_hello").is_some());
        assert!(skills.get("demo-skill").is_some());
        assert!(agents.get("demo-agent").is_some());

        deactivate_plugin(&tools, &skills, &agents, &extensions, &hook_registry, "com.example.demo");
        assert!(tools.get_def("demo_hello").is_none());
        assert!(skills.get("demo-skill").is_none());
        assert!(agents.get("demo-agent").is_none());
        assert!(!extensions
            .remove_message_loop_prompts_after(&plugin_rules_hook_key("com.example.demo")));
    }

    #[test]
    fn activate_plugin_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugins");
        let plugin_dir = write_example_plugin(&root, "com.example.demo").unwrap();
        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);
        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        reg.authorize("com.example.demo").unwrap();
        reg.enable("com.example.demo").unwrap();
        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        let record = reg.get("com.example.demo").unwrap();

        let (tools, skills, agents, extensions, hook_registry) = build_registries();
        activate_plugin(&tools, &skills, &agents, &extensions, &hook_registry, &record).unwrap();
        activate_plugin(&tools, &skills, &agents, &extensions, &hook_registry, &record).unwrap();
        assert!(tools.get_def("demo_hello").is_some());
        assert!(skills.get("demo-skill").is_some());
        assert!(agents.get("demo-agent").is_some());
        assert_eq!(tools.tool_names_by_plugin("com.example.demo").len(), 1);
    }

    #[tokio::test]
    async fn rules_hook_injects_content() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugins");
        let plugin_dir = write_example_plugin(&root, "com.example.demo").unwrap();
        let auth_file = tmp.path().join("auth.json");
        let reg = PluginRegistry::with_auth_path(auth_file);
        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        reg.authorize("com.example.demo").unwrap();
        reg.enable("com.example.demo").unwrap();
        reg.scan_roots(&[(plugin_dir.clone(), true)]).unwrap();
        let record = reg.get("com.example.demo").unwrap();

        let (tools, skills, agents, extensions, hook_registry) = build_registries();
        activate_plugin(&tools, &skills, &agents, &extensions, &hook_registry, &record).unwrap();

        let computer = ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = MessageLoopPromptsAfterContext {
            computer_state: &computer,
            lead_agent_profile: AgentProfile::General,
            base_messages: base,
            injected_tail: &mut tail,
            conversation_id: "test",
            stream: None,
            round_assistant_message_id: None,
            round_screen_dump_prefix: None,
            task_board_store: Arc::new(crate::task_board::TaskBoardStore::new()),
            task_board_store_key: "test",
            user_dynamic_inject_enabled: true,
            workspace_root: "",
        };

        // 取出 rules hook 手动执行（模拟 run_message_loop_prompts_after）
        let hooks: Vec<Arc<dyn MessageLoopPromptsAfterHook>> = extensions.hooks_snapshot();
        assert!(!hooks.is_empty());
        for h in hooks {
            h.execute(&mut ctx).await.unwrap();
        }
        let injected = tail
            .iter()
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(injected.contains("插件规则"), "injected: {injected}");
        assert!(injected.contains("guardrail.md"));
    }
}
