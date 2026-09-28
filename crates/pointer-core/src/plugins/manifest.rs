//! `pointer-plugin.toml` 解析与校验（Pointer 原生插件 · 唯一运行时格式）。
//!
//! 设计稿 §4.1：插件 manifest 是插件包的「分发箱」，能力单元（skills/agents/hooks/
//! mcp_servers/tools）在启用时分别接入现有 Registry。P1 校验铁律（审查注 4.1.1）：
//! `[[tools.tool]]` 必须携带 `exec` 执行载体（sidecar 或 MCP server），只有元数据
//! 而无执行器的工具声明为无效配置。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 插件元信息段 `[plugin]`。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginMeta {
    /// 反向域名，全局唯一（如 `com.example.demo`）。
    pub id: String,
    pub name: String,
    pub version: String,
    /// 声明式 API 版本；当前仅支持 `v1`。
    #[serde(default = "default_api_version")]
    pub api_version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
}

fn default_api_version() -> String {
    "v1".to_string()
}

/// 权限声明 `[permissions]`。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PluginPermissions {
    /// 网络域名白名单（P2 进程包装层实施或审批兜底）。
    #[serde(default)]
    pub network: Vec<String>,
    /// 文件系统路径范围（P1 复用工作区边界 + 审批）。
    #[serde(default)]
    pub filesystem: Vec<String>,
    /// 进程环境变量白名单。
    #[serde(default)]
    pub env: Vec<String>,
    /// 声明式密钥引用（`${secrets.X}` 启动时注入）。
    #[serde(default)]
    pub secrets: Vec<String>,
}

/// 子目录引用（skills / agents 等）。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginDirRef {
    pub path: String,
}

/// hooks 声明 `[hooks]`（P3 执行器；P1 仅解析保留）。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginHooksRef {
    pub path: String,
}

/// MCP server 声明容器 `[mcp_servers]`，内部 `[[mcp_servers.server]]`。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpServersDecl {
    #[serde(default, rename = "server")]
    pub server: Vec<McpServerDecl>,
}

/// MCP server 声明 `[[mcp_servers.server]]`（P2 接入；P1 解析校验但不启动）。
/// `transport=stdio` 时用 `command/args/env` 启动本地进程；
/// `transport=http`（streamable HTTP）时用 `url` 连接远程服务（客户端场景）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDecl {
    pub name: String,
    #[serde(default = "default_transport_stdio")]
    pub transport: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// http 传输：远程服务地址（必填）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// http 传输：附加请求头（如 Authorization: Bearer ...）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

fn default_transport_stdio() -> String {
    "stdio".to_string()
}

/// `[[tools.tool]]` 执行载体：sidecar 进程（P1）或 MCP server 引用（P2）。
/// 二选一必填；两者皆缺 = 无效配置。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginToolExec {
    /// 可执行命令（相对插件根目录或绝对路径）。
    pub command: String,
    /// `sidecar`（P1 支持）或 `mcp`（P2 支持；`server` 必填）。
    #[serde(default = "default_transport_sidecar")]
    pub transport: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// MCP transport 时引用的 `[[mcp_servers.server]].name`。
    #[serde(default)]
    pub server: Option<String>,
    /// 进程超时（毫秒）。
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_transport_sidecar() -> String {
    "sidecar".to_string()
}

/// `[[tools.tool]]` 进程外工具声明。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginToolDecl {
    /// 工具名（注册进 `ToolRegistry` 的 def.name）。
    pub name: String,
    /// `low` / `medium` / `high`；默认 `low`。
    #[serde(default = "default_risk_level")]
    pub risk_level: String,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub parallel_eligible: bool,
    /// 工具说明（生成 doc_markdown 用）。
    #[serde(default)]
    pub description: String,
    /// 参数 JSON Schema（可选；缺省时工具描述只给说明文本）。
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
    /// 执行载体（必填；无 exec 直接 rejected）。
    #[serde(default)]
    pub exec: Option<PluginToolExec>,
}

/// `[[tools.tool]]` 容器 `[tools]`。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PluginToolsDecl {
    #[serde(default, rename = "tool")]
    pub tool: Vec<PluginToolDecl>,
}

fn default_risk_level() -> String {
    "low".to_string()
}

/// 顶层插件 manifest。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    #[serde(rename = "plugin")]
    pub plugin: PluginMeta,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub skills: Option<PluginDirRef>,
    #[serde(default)]
    pub agents: Option<PluginDirRef>,
    /// 规则目录（`*.mdc` / `*.md`，注入 ExtensionRegistry 的 MessageLoopPromptsAfter）。
    #[serde(default)]
    pub rules: Option<PluginDirRef>,
    #[serde(default)]
    pub hooks: Option<PluginHooksRef>,
    #[serde(default, rename = "mcp_servers")]
    pub mcp_servers: McpServersDecl,
    #[serde(default, rename = "tools")]
    pub tools: PluginToolsDecl,
    /// 导入转换器无法映射的字段保留在此（导入报告标注「未映射」）。
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

impl PluginManifest {
    /// 校验插件 id 是否为反向域名（`[a-z0-9-]+(\.[a-z0-9-]+)+`）。
    pub fn validate(&self) -> Result<()> {
        let meta = &self.plugin;
        if meta.id.trim().is_empty() {
            return Err(anyhow!(
                "[plugin].id 不能为空（建议反向域名，如 com.example.demo）"
            ));
        }
        if !is_reverse_domain(&meta.id) {
            return Err(anyhow!(
                "[plugin].id `{}` 必须是反向域名（如 com.example.demo，仅小写字母/数字/连字符/点）",
                meta.id
            ));
        }
        if meta.name.trim().is_empty() {
            return Err(anyhow!("[plugin].name 不能为空"));
        }
        if meta.version.trim().is_empty() {
            return Err(anyhow!("[plugin].version 不能为空"));
        }
        if meta.api_version != "v1" {
            return Err(anyhow!(
                "[plugin].api_version `{}` 不受支持（当前仅支持 v1）",
                meta.api_version
            ));
        }

        // 执行载体必填：P1 只支持 sidecar；mcp 需 P2。
        for tool in &self.tools.tool {
            if tool.name.trim().is_empty() {
                return Err(anyhow!("[[tools.tool]] 存在空 name"));
            }
            let Some(exec) = &tool.exec else {
                return Err(anyhow!(
                    "[[tools.tool]].name=`{}` 缺少 exec 执行载体（sidecar 或 mcp server 二选一必填）",
                    tool.name
                ));
            };
            if exec.command.trim().is_empty() {
                return Err(anyhow!(
                    "[[tools.tool]].name=`{}` 的 exec.command 不能为空",
                    tool.name
                ));
            }
            match exec.transport.as_str() {
                "sidecar" => {}
                "mcp" => {
                    if exec.server.as_deref().is_none_or(str::is_empty) {
                        return Err(anyhow!(
                            "[[tools.tool]].name=`{}` 使用 mcp transport 时 exec.server 必填（引用 [[mcp_servers.server]].name）",
                            tool.name
                        ));
                    }
                }
                other => {
                    return Err(anyhow!(
                        "[[tools.tool]].name=`{}` 的 exec.transport `{other}` 不受支持（sidecar | mcp）",
                        tool.name
                    ));
                }
            }
            if !matches!(tool.risk_level.as_str(), "low" | "medium" | "high") {
                return Err(anyhow!(
                    "[[tools.tool]].name=`{}` 的 risk_level `{}` 不受支持（low | medium | high）",
                    tool.name,
                    tool.risk_level
                ));
            }
        }

        for server in &self.mcp_servers.server {
            if server.name.trim().is_empty() {
                return Err(anyhow!("[[mcp_servers.server]] 存在空 name"));
            }
            if server.command.trim().is_empty() {
                return Err(anyhow!(
                    "[[mcp_servers.server]].name=`{}` 的 command 不能为空",
                    server.name
                ));
            }
            if !matches!(server.transport.as_str(), "stdio" | "http") {
                return Err(anyhow!(
                    "[[mcp_servers.server]].name=`{}` 的 transport `{}` 不受支持（stdio | http）",
                    server.name,
                    server.transport
                ));
            }
        }
        Ok(())
    }
}

fn is_reverse_domain(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// 解析 `pointer-plugin.toml` 文本并校验。
pub fn parse(raw: &str) -> Result<PluginManifest> {
    let manifest: PluginManifest =
        toml::from_str(raw).map_err(|e| anyhow!("pointer-plugin.toml 解析失败: {e}"))?;
    manifest.validate()?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
[plugin]
id = "com.example.demo"
name = "DEMO 报销工具"
version = "1.2.0"
api_version = "v1"
description = "示例 报销提交与预审"
author = "example"
license = "MIT"

[permissions]
network = ["https://plugin.example.com"]
filesystem = ["workspace:read"]
secrets = ["DEMO_TOKEN"]

[skills]
path = "skills/"

[agents]
path = "agents/"

[rules]
path = "rules/"

[[mcp_servers.server]]
name = "demo"
transport = "stdio"
command = "bin/demo-mcp"
args = ["serve"]
env = { DEMO_TOKEN = "${secrets.DEMO_TOKEN}" }

[[tools.tool]]
name = "demo_submit"
risk_level = "high"
requires_approval = true
parallel_eligible = false
exec = { command = "bin/demo-tool", transport = "sidecar" }
"#;

    #[test]
    fn parses_valid_manifest() {
        let m = parse(VALID).unwrap();
        assert_eq!(m.plugin.id, "com.example.demo");
        assert_eq!(m.plugin.api_version, "v1");
        assert_eq!(m.permissions.secrets, vec!["DEMO_TOKEN"]);
        assert_eq!(m.skills.as_ref().unwrap().path, "skills/");
        assert_eq!(m.agents.as_ref().unwrap().path, "agents/");
        assert_eq!(m.rules.as_ref().unwrap().path, "rules/");
        assert_eq!(m.mcp_servers.server.len(), 1);
        assert_eq!(m.mcp_servers.server[0].name, "demo");
        assert_eq!(m.tools.tool.len(), 1);
        assert_eq!(m.tools.tool[0].name, "demo_submit");
        assert_eq!(m.tools.tool[0].risk_level, "high");
        assert!(m.tools.tool[0].requires_approval);
        let exec = m.tools.tool[0].exec.as_ref().unwrap();
        assert_eq!(exec.command, "bin/demo-tool");
        assert_eq!(exec.transport, "sidecar");
    }

    #[test]
    fn rejects_missing_id() {
        let raw = VALID.replace("id = \"com.example.demo\"\n", "");
        let err = parse(&raw).unwrap_err().to_string();
        assert!(err.contains("id"), "unexpected: {err}");
    }

    #[test]
    fn rejects_bad_api_version() {
        let raw = VALID.replace("api_version = \"v1\"", "api_version = \"v2\"");
        let err = parse(&raw).unwrap_err().to_string();
        assert!(err.contains("api_version"), "unexpected: {err}");
    }

    #[test]
    fn rejects_tool_without_exec() {
        let raw = VALID.replace(
            "exec = { command = \"bin/demo-tool\", transport = \"sidecar\" }\n",
            "",
        );
        let err = parse(&raw).unwrap_err().to_string();
        assert!(err.contains("缺少 exec"), "unexpected: {err}");
    }

    #[test]
    fn rejects_bad_transport() {
        let raw = VALID.replace(
            "exec = { command = \"bin/demo-tool\", transport = \"sidecar\" }",
            "exec = { command = \"bin/demo-tool\", transport = \"docker\" }",
        );
        let err = parse(&raw).unwrap_err().to_string();
        assert!(err.contains("transport"), "unexpected: {err}");
    }

    #[test]
    fn rejects_mcp_transport_without_server_ref() {
        let raw = VALID.replace(
            "exec = { command = \"bin/demo-tool\", transport = \"sidecar\" }",
            "exec = { command = \"bin/demo-mcp\", transport = \"mcp\" }",
        );
        let err = parse(&raw).unwrap_err().to_string();
        assert!(err.contains("exec.server"), "unexpected: {err}");
    }

    #[test]
    fn accepts_mcp_transport_with_server_ref() {
        let raw = VALID.replace(
            "exec = { command = \"bin/demo-tool\", transport = \"sidecar\" }",
            "exec = { command = \"bin/demo-mcp\", transport = \"mcp\", server = \"demo\" }",
        );
        let m = parse(&raw).unwrap();
        let exec = m.tools.tool[0].exec.as_ref().unwrap();
        assert_eq!(exec.transport, "mcp");
        assert_eq!(exec.server.as_deref(), Some("demo"));
    }

    #[test]
    fn rejects_bad_risk_level() {
        let raw = VALID.replace("risk_level = \"high\"", "risk_level = \"ultra\"");
        let err = parse(&raw).unwrap_err().to_string();
        assert!(err.contains("risk_level"), "unexpected: {err}");
    }

    #[test]
    fn reverse_domain_validation() {
        assert!(is_reverse_domain("com.example.demo"));
        assert!(is_reverse_domain("io.github.user"));
        assert!(!is_reverse_domain("demo"));
        assert!(!is_reverse_domain("com.example."));
        assert!(!is_reverse_domain(".com.example"));
        assert!(!is_reverse_domain("com..example"));
        assert!(!is_reverse_domain("Com.Example"));
    }
}
