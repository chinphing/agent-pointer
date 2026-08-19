# Pointer 插件开发指南（用户级）

> 面向插件作者。本文档讲**如何从零编写一个 Pointer 原生插件**（`pointer-plugin.toml`），
> 以及如何把已有的 Codex / Claude Code 插件导入为 Pointer 原生格式。
> 插件机制实现位置：`crates/pointer-core/src/plugins/`（manifest / importer / activation / registry）。

---

## 1. 插件是什么

Pointer 插件是一个**目录**，包含：

- `pointer-plugin.toml` —— 唯一运行时清单（**原生格式，必须存在**）
- 若干**能力单元**子目录：`skills/`、`agents/`、`rules/`、`tools`（内嵌声明）、`hooks/`、`mcp_servers`（内嵌声明）

启用插件后，它的能力单元自动接入 Pointer 的注册表：

| 能力单元 | 说明 | 现状 |
|---|---|---|
| `skills/` | SKILL.md 技能（与用户技能同格式） | ✅ 启用即注册，自动加入通用助手启用列表 |
| `agents/` | AGENT.md 子代理（worker） | ✅ 启用即注册 |
| `rules/` | 规则文件（`.md` / `.mdc`），每轮对话注入 | ✅ 启用即注入 |
| `[[tools.tool]]` | 进程外工具（sidecar 可执行文件） | ✅ 启用即注册（**须带 `exec` 执行载体**） |
| `hooks/` | hooks.json（Claude 语义） | ✅ 启用即注册（PreToolUse 阻断 / PostToolUse 观察；脚本失败默认放行，entry 可配 `fail_closed` 切换为阻断） |
| `mcp_servers` | MCP server 声明 | ✅ 启用即启动（stdio JSON-RPC，工具自动注册；崩溃自动重启，重试达上限插件状态显示「运行异常」）。另支持**全局 MCP**（非插件）：`pointer-server.toml` 配置 `[[mcp_servers.server]]`，工具命名 `mcp.<server>.<tool>`，设置面板「MCP」分区管理 |

> 插件导入后是一次性**快照**：源目录后续变更不会自动同步，需要重新导入。

---

## 2. 目录结构

```text
my-plugin/
├── pointer-plugin.toml        # 必需：插件清单
├── skills/                    # 可选：技能（每个子目录含 SKILL.md）
│   └── hello/
│       └── SKILL.md
├── agents/                    # 可选：子代理（每个子目录含 AGENT.md）
│   └── worker/
│       └── AGENT.md
├── rules/                     # 可选：规则（.md / .mdc，递归收集）
│   └── guardrail.md
├── hooks/                     # 可选：hooks.json（PreToolUse / PostToolUse）
│   └── hooks.json
├── bin/                       # 约定：sidecar 可执行文件放这里
│   └── demo-tool
└── .mcp.json                  # 可选：MCP 声明（Claude 约定，P2）
```

插件 id 是**反向域名**（`com.example.cwpt`），安装到 `~/.pointer/plugins/<id>/`。

---

## 3. pointer-plugin.toml 参考

```toml
# ── 必填段 ──────────────────────────────────────────────
[plugin]
id = "com.example.cwpt"        # 反向域名，全局唯一，仅小写字母/数字/连字符/点
name = "示例插件"               # 必填
version = "1.0.0"              # 必填
api_version = "v1"             # 可选，默认 v1（当前仅支持 v1）
description = "做某件事"        # 可选
author = "作者"                 # 可选
license = "MIT"                # 可选

# ── 权限声明（可选，P1 用于审批兜底）────────────────────
[permissions]
network = []                   # 网络域名白名单（P2 进程包装层实施）
filesystem = []                # 文件系统路径范围
env = []                       # 进程环境变量白名单
secrets = []                   # 声明式密钥引用 ${secrets.X}

# ── 能力单元目录（可选）─────────────────────────────────
[skills]
path = "skills/"

[agents]
path = "agents/"

[rules]
path = "rules/"

[hooks]
path = "hooks/"

# ── MCP server 声明（可选，P2 启动；当前解析校验）────────
[mcp_servers]
[[mcp_servers.server]]
name = "demo"
transport = "stdio"            # 默认 stdio
command = "bin/demo-mcp"
args = ["serve"]
env = { KEY = "value" }

# ── 进程外工具（可选）────────────────────────────────────
[tools]
[[tools.tool]]
name = "demo_hello"            # 工具名（注册进 ToolRegistry）
risk_level = "low"             # low | medium | high，默认 low
requires_approval = false
parallel_eligible = false
description = "问候演示工具"
# 参数 JSON Schema（可选；缺省时仅给说明文本）
schema = { type = "object", properties = { name = { type = "string" } } }

# 执行载体：sidecar 或 mcp 二选一（必填，缺 exec 会被拒绝）
[[tools.tool.exec]]
command = "bin/demo-tool"      # 相对插件根目录
transport = "sidecar"          # sidecar（P1）| mcp（P2，须指定 server）
timeout_ms = 30000             # 可选
env = { KEY = "value" }        # 可选

# ── 导入转换器无法映射的字段保留在此（导入报告标注「未映射」）──
[metadata]
original = "…"
```

### 校验规则（不符合会被标记 `rejected`，无法启用）

- `[plugin].id` 必须是反向域名，非空；
- `name` / `version` 非空；`api_version` 仅支持 `v1`；
- `[[tools.tool]]` **必须有 `exec`**（sidecar 或 mcp server 二选一）；
- `exec.transport` 仅 `sidecar` | `mcp`；`mcp` 时必须指定 `exec.server`（引用 `[[mcp_servers.server]].name`）；
- `risk_level` 仅 `low` | `medium` | `high`。

---

## 4. 从零写一个插件（示例）

```bash
# 1. 建目录
mkdir -p ~/dev/my-plugin/{skills/hello,agents/worker,rules,bin}

# 2. 写清单（见上节，id 用反向域名）
cat > ~/dev/my-plugin/pointer-plugin.toml <<'EOF'
[plugin]
id = "com.example.cwpt"
name = "示例插件"
version = "1.0.0"
description = "演示能力单元"

[skills]
path = "skills/"

[agents]
path = "agents/"

[rules]
path = "rules/"
EOF

# 3. 写技能（标准 SKILL.md，frontmatter 含 name/description）
cat > ~/dev/my-plugin/skills/hello/SKILL.md <<'EOF'
---
name: hello
description: 打招呼示例技能
---
当任务匹配时调用本技能……
EOF

# 4. 写子代理（标准 AGENT.md）
cat > ~/dev/my-plugin/agents/worker/AGENT.md <<'EOF'
---
id: worker
name: 示例 Worker
description: 演示子代理
role: worker
enabled: true
---
You are a demo worker agent.
EOF

# 5. 写规则
cat > ~/dev/my-plugin/rules/guardrail.md <<'EOF'
# 守则
始终用中文回复。
EOF

# 6. 打包（可选，zip 和目录都支持导入）
cd ~/dev && zip -r my-plugin.zip my-plugin
```

**导入方式**（三选一）：

- 桌面端设置 → 插件 → 「选择目录导入」或「导入 zip」；
- Web 端设置 → 插件 → 「导入」填目录/zip 路径，或「上传 zip」；
- API：`POST /api/plugins`（路径）或 `POST /api/plugins/import-zip`（zip 字节）。

导入后到插件页「启用」。启用时自动：

- 注册 skills/agents/rules/tools 到对应 Registry；
- 把插件技能自动加入**通用助手**的启用列表（技能面板显示「插件 · 插件名」徽标）；
- 禁用插件 → 能力注销（启用列表保留，重新启用即恢复）；
- 卸载插件 → 能力注销 + 从启用列表移除。

---

## 5. sidecar 工具怎么返回结果

`sidecar` 工具是插件内的可执行文件。Pointer 调用它时：

- `exec.command` 相对插件根目录（或绝对路径）解析；
- 工具输出约定：stdout 按 **JSON** 返回给模型，形如：

```bash
#!/bin/sh
# 读 stdin 里的工具参数（JSON），处理，然后输出 JSON
echo '{"ok": true, "result": "hello"}'
```

（示例：`crates/pointer-core/src/plugins/activation.rs` 测试里的 `bin/demo-tool`。）

> P1 的 `ToolHandler` 是同步闭包；sidecar 以独立进程执行，超时由 `timeout_ms` 控制。

---

## 6. 从 Codex / Claude 插件导入

Pointer 不直接运行时加载 Codex / Claude 格式，而是**一次性转换**为原生格式：

- **Claude 插件**：`.claude-plugin/plugin.json` + `skills/` `agents/` `commands/` `hooks/` `.mcp.json`
- **Codex 插件**：`.codex-plugin/plugin.json`（或仓库根 `plugin.json`）+ `skills/`（部分在 `.agents/` 等）

导入时自动按 **Pointer → Codex → Claude** 优先级识别，输出报告：

```text
「superpowers」（local.superpowers）：skills、hooks、pointer-plugin.toml；跳过 agents、commands；未映射 1 项
```

- `converted`：已转换的能力单元（如 `skills`、`commands→skills`、`mcp_servers`）；
- `skipped`：源中不存在或不支持的部分（如 `agents`、`commands`）；
- `unmapped`：manifest 中无法映射的字段（保留在 `[metadata]`，不丢信息）。

> commands 在 Pointer 中并入技能（生成 `skills/<name>/SKILL.md`）；hooks 启用后执行
> （PreToolUse 可阻断、PostToolUse 观察；`"fail_closed": true` 时脚本失败/超时会阻断工具调用）；MCP 解析保留暂不启动。导入后请以「启用 + 技能面板核对」验证实际能力。

---

## 7. 插件目录位置与生命周期

| 项 | 说明 |
|---|---|
| 用户级插件 | `~/.pointer/plugins/<id>/` |
| 授权状态 | `~/.pointer/plugins/.auth.json`（enabled / enabled_at / fingerprint） |
| 来源标注 | 技能面板徽标：`插件 · 插件名`；卸载后从启用列表移除 |
| 幂等 | 重复导入同 id 会先删除旧目录再写入；重复启用安全 |
| 快照语义 | 导入是一次性拷贝，源目录后续变更不会自动同步 |

### 插件状态机

```text
discovered（发现） → needs_reauth（hash 变化） → enabled（启用）
        └── rejected（校验失败，无法启用）
enabled → disabled（禁用，能力注销） → enabled（重新启用）
enabled → uninstall（卸载：删目录 + 清授权 + 移除技能）
```

---

## 8. FAQ

**Q: 插件技能为什么自动启用？**
启用插件时，其技能自动加入通用助手的启用列表（落盘）。每次刷新技能列表/启动也会兜底对齐。
禁用插件不清理启用列表，重新启用即恢复；卸载会清理。

**Q: 为什么我的插件被标记 rejected？**
最常见：`[plugin].id` 不是反向域名、工具缺 `exec`、`exec.transport` 不是 `sidecar`/`mcp`、`risk_level` 非法。
导入报告会给出具体原因。

**Q: 我的技能目录为什么没生效？**
检查 `[skills] path = "skills/"` 指向的目录下每个技能是**独立子目录**且含 `SKILL.md`；
技能 frontmatter 需要 `name` 和 `description`。

**Q: hooks / MCP 什么时候能跑？**
hooks 执行器已实现（PreToolUse / PostToolUse，sidecar 进程 + JSON 决策）；MCP 接入（P2）仍在路线图中。

**Q: 插件更新了源目录，Pointer 里还是旧的？**
插件是快照。请重新导入（同 id 覆盖）或手动更新 `~/.pointer/plugins/<id>/` 后重启/重新启用。
