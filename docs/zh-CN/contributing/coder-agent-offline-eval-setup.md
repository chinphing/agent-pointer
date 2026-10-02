# Coder 离线验证环境：本地搭建指南

[English](../../en/contributing/coder-agent-offline-eval-setup.md) | 简体中文

> **读者**：要在本机跑「可重复判分」的回归任务，评估 prompt / 模型 / 工具改动。  
> **说明**：「离线」在下面分 **三层** 理解，可按需只做其中一层。

---

## 1. 「离线」指什么（先对齐预期）

| 层级 | 含义 | 典型实现 |
|------|------|----------|
| **A. 判分离线** | 不依赖人工点击 UI；通过/失败由脚本判定 | Shell / `cargo test` / 文件 grep |
| **B. Runner 离线** | 任务环境与依赖可复现，不依赖你们线上服务 | 固定 Git tag、Docker 或 `cargo` 锁版本 |
| **C. LLM 离线** | 模型推理不访问公网 API | 本机 **Ollama** / **vLLM** 等，OpenAI 兼容 `http://127.0.0.1:…/v1` |

Pointer 默认走 **DashScope 等云端**；要做 **C**，在应用「设置」里把 Base URL 与模型名改成本地兼容端点即可（仍可能访问本机以外的局域网，视部署而定）。

---

## 2. 最小可行方案（建议第一步）：「金样任务 + 脚本判分」

不接完整 Agent 也能先搭好 **A + B** 的一半：验证「仓库状态 → 命令 → 退出码/输出」链路。

### 2.1 目录建议（与主仓库并列或子目录均可）

```text
eval/
  fixtures/
    tiny-rust-bug/          # 小型 Rust 工程，或 git submodule 指向固定 tag
      Cargo.toml
      src/lib.rs            # 故意留一个可测的失败点
  cases/
    case-001.toml           # 用例元数据：描述、工作目录、判分命令
  scripts/
    grade.sh                # 统一入口：准备环境 → 跑判分 → 输出 JSON
```

### 2.2 单个用例元数据示例（`case-001.toml`）

字段仅为示例，可按团队习惯改名：

```toml
id = "case-001"
description = "Fix add() off-by-one"

[workspace]
# 判分时复制 fixture 到此临时目录，避免污染 fixture 源目录
template = "fixtures/tiny-rust-bug"

[[grade]]
command = ["cargo", "test", "-q"]
cwd = "."
expect_exit_code = 0
```

### 2.3 `grade.sh` 逻辑要点

1. `tmpdir=$(mktemp -d)`，将 `fixtures/...` **rsync/cp** 到 `$tmpdir`。  
2. 若用例描述为「模型应产出 patch」，可先把 **golden patch** 或 **agent 输出 patch** 应用到 `$tmpdir`（`git apply` 或 `patch -p1`）。  
3. 在 `$tmpdir` 内执行 `grade.command`，检查 **退出码** 与（可选）**stdout 子串**。  
4. 打印一行 JSON：`{"id":"case-001","pass":true}` 便于 CI 收集。

**收益**：完全 **A 判分**；不依赖 Pointer UI；Docker 可选。  
**与 Pointer 的关系**：此层验证「任务定义是否合理」；Agent 接入见 §4 接入 Pointer：用本地 Web Server + API 驱动 Coder。

---

## 3. 用 Docker 固定 Runner（B 加强版）

当依赖是 **多版本 Node / 系统库** 或要避免污染宿主机时：

1. 为 `fixtures/tiny-rust-bug` 写 `Dockerfile`：固定 `FROM rust:1.xx-bookworm`，安装 **仅判分需要** 的工具。  
2. `docker build -t pointer-eval:case001 .`  
3. `docker run --rm -v "$PWD/out:/out" pointer-eval:case001 cargo test -q`

**与 SWE-bench 的关系**：SWE-bench 也是「一实例一容器 + 测过」；你们自研用例可保持同样习惯，日后迁移到 SWE 官方 harness 时心智一致。参考：[SWE-bench](https://github.com/swe-bench/swe-bench)。

---

## 4. 接入 Pointer：用本地 Web Server + API 驱动 Coder

主仓库已提供 **HTTP + SSE**（见 `server/src/main.rs`），适合写脚本驱动多轮对话（需消费 SSE 解析最终消息与工具结果）。

### 4.1 启动本地 Core

```bash
cd /path/to/pointer-app
npm install
npm run server:dev
```

（与 README 中 Web 开发方式一致；端口以终端输出为准，下文假设 `http://127.0.0.1:3000`。）

### 4.2 配置 API 与模型

1. 浏览器打开 Web 端或调用 `PUT /api/settings`（见服务端路由）设置 **`workspaceRoot`** 指向 **§2 最小可行方案（建议第一步）：「金样任务 + 脚本判分」里临时副本** 或专用小仓库。  
2. **云端模型**：`POST /api/key` 写入 Key；或  
3. **本机模型（C）**：在设置里将 **Base URL** 设为例如 `http://127.0.0.1:11434/v1`（Ollama OpenAI 兼容），模型名填 Ollama 中已有名称。

### 4.3 发起一轮对话

- **接口**：`POST /api/chat`，请求体为 `SendChatPayload`（`crates/pointer-core/src/models.rs`）：  
  - `conversationId`：固定测试会话 id。  
  - `messages`：`ChatMessage` 数组（至少包含一条用户消息；字段与前端一致，含 `role`/`content` 等）。  
  - `agentMode`：设为 coder 对应模式字符串（与前端选择 Coder 时一致；可通过 `GET /api/agents` 核对列表与 id）。  
  - `enabledSkillIds` / `toolRoundsUsed`：按需要传，与线上一致即可。

- **流式结果**：`GET /api/chat/:conversation_id/stream`（SSE），脚本需解析事件直至 `Done` 或工具轮结束，再从持久化对话或事件中抽取 **最终工作区是否被修改**，再回到 **§2 最小可行方案（建议第一步）：「金样任务 + 脚本判分」的 `grade.sh`** 做判分。

**说明**：仓库内 **尚无** 现成「一条 CLI 跑完 eval」命令；自动化通常是自己写 **Python/Node 小脚本** 调上述 API + SSE。若未来在 `examples/` 或 `eval/` 增加官方 runner，可作为后续实现项（见 [`coder-agent-capability-roadmap.md`](../design/coder-agent-capability-roadmap.md) §5.11 L — 离线评测集）。

### 4.4 工具审批

若设置中敏感工具需审批，`POST /api/tools/:tool_call_id/approve`。脚本里需在收到「待审批」类事件后自动 `approve: true`，才能实现无人值守回归。

---

## 5. 判分策略清单（从易到难）

| 判分类型 | 做法 | 适用 |
|----------|------|------|
| 退出码 | `cargo test`、`npm test` | 首选 |
| 输出子串 | `grep -q` on stdout/stderr | 快速断言 |
| 文件内容 | `rg` / `test -f` / `sha256sum` 与 golden 对比 | 检查是否改对文件 |
| 禁止改动 | 对不应触碰的路径 `git diff --name-only` 为空 | 防跑题 |

---

## 6. 与 CI 的关系

- **不强制**：本地 `eval/scripts/grade.sh` 跑通即可。  
- **可选**：在 GitHub Actions 中加 job：缓存 Docker 层、只跑 **Lite 子集**（例如 3～5 个 case），避免阻塞主 CI。

---

## 7. 常见问题

**Q：没有 API Key 能否测 Agent？**  
A：可测 **§2 最小可行方案（建议第一步）：「金样任务 + 脚本判分」判分链路**；要测真实模型调用需 **本机模型（C）** 或内网网关，否则仍需 Key。

**Q：能否完全断网？**  
A：**判分脚本**可以；**云端 LLM** 不能。完全断网需 **C**。

**Q：和 SWE-bench 二选一吗？**  
A：不互斥。自研 `eval/` 管 **你们产品回归**；SWE-bench 管 **横向对标**；后者见路线图 §5.11 L — 离线评测集 及前文讨论。

---

## 8. 维护建议

- Fixture 用 **固定 tag 的 submodule** 或 **vendor 最小 tarball**，避免「上游 main 变了 case 飘」。  
- 每个 case 在 CI 中 **超时**（如 15～30 分钟上限），防止模型死循环拖死 runner。

---

**相关文档**：[`coder-agent-capability-roadmap.md`](../design/coder-agent-capability-roadmap.md)（§5.11 L — 离线评测集、度量 §7）
