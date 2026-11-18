# 执行环境（Environment）与沙箱化设计 v0.3

> 状态：草案，待评审
> 范围：agent 执行环境抽象、local / light / container 三档实现
> 不在范围：模型接入、UI 交互、计费
> 最近更新：2026-11-15（补齐实测数据与本地开发环境）

## 文档导航

| 你想做什么 | 看哪节 |
|---|---|
| **先跑起来** | §9.0 从零准备（10 分钟）→ §9.1 脚本 |
| **开始写代码** | §7.1 实现顺序 → §4.2 接口定义 |
| 理解为什么这么设计 | §2 核心决策 → §2.3 性能实测 |
| 部署 light 到生产 | §5 阶段二 → §9.2 能验/不能验 |
| 做 GUI | §6 阶段三 |
| 换后端 / 防锁死 | §3.2 三原则 |
| **插件 / 媒体 / 桌面进程算不算沙箱内** | §3.3 进程边界 |
| **admin 为什么默认 local / 怎么指定 admin** | §3.4 账号与执行环境 |

**一句话现状**：抽象层与协议已定，本地开发环境已跑通（真实 E2B envd），P1 可以直接开工；Cube 相关的控制面延迟与 Volume 语义仍需真机验证。

---

## 0. 目标

把 agent 对本机的一切操作收敛到一个接口层，背后提供三档隔离：

| 档位 | 隔离强度 | 桌面 | 典型用途 |
|---|---|---|---|
| **local** | 无 | 宿主真实桌面 | 单机开发、可信任务 |
| **light** | namespace + uid + 挂载 | 无 | Web 多用户、文件互不可见 |
| **container** | 同上 + 完整运行时 | 有（Xvfb + WM） | GUI 自动化 |

**当前最高优先级是 light**：Web 端多用户并发使用，核心诉求是

1. 用户工作区文件互不可见
2. skills 目录只读，普通用户不能改
3. 不需要 GUI

---

## 1. 现状勘察

改造前先确认已有的接缝，避免重复造轮子。

### 1.1 提示词已经是模板

`crates/pointer-core/src/agents/mod.rs:513`

```rust
template.replace("{{workspace_root}}", vars.workspace_root)
```

系统提示词里已经是 `**Workspace root** (absolute path from app settings): \`{{workspace_root}}\``。

**结论**：workspaceRoot 不需要改文案，只需要在构建 `vars` 时换值。

### 1.2 路径解析已集中

`crates/pointer-core/src/tools/file/path.rs` 提供 5 个入口：

| 函数 | 用途 |
|---|---|
| `resolve_relative_under_workspace` | 相对路径基准 |
| `resolve_writable_path` | 写操作 |
| `resolve_within_workspace_root` | 边界校验 |
| `resolve_existing_read_path` | 读操作 |
| `resolve_accessible_path` | 通用可达路径 |

已有测试覆盖绝对路径场景（`resolve_accepts_absolute_path_under_workspace` / `resolve_rejects_absolute_path_outside_workspace`）。

**结论**：路径策略的插入点唯一。

### 1.3 已有用户沙箱模型

`crates/pointer-core/src/session_sandbox.rs`

```text
{session-sandboxes}/{session_user_id}/              用户非空时
{session-sandboxes}/_anonymous/{conversation_id}/   用户为空时
```

**结论**：per-user 目录已经是既有概念，沙箱化不需要重新发明。

### 1.4 真正的成本在 IO 层

路径解析之后，所有工具直接打本地文件系统：

| 模块 | 现状 |
|---|---|
| `tools/file/{read,write,edit,list,glob,grep}.rs` | `std::fs` |
| `workspace_read.rs`（文件树 / git status / diff） | `Path` + `std::fs` + git |
| `turn_file_baseline.rs` | 本地快照对比 |
| `worktree/` | 本地 git worktree |
| terminal 工具 | 本地 `Command` spawn |
| `read_lints` | 本地跑编译器 |

**结论**：路径映射便宜，IO 抽象昂贵。设计上必须把两者解耦。

---

## 2. 核心决策：路径映射

### 2.1 两种方案

| | A 路径保真挂载 | B 路径映射 + IO 抽象 |
|---|---|---|
| 做法 | 容器内挂到与宿主**完全相同的绝对路径** | 入口把宿主路径翻译成容器路径 |
| `std::fs` 改动 | **零** | 6+ 模块全部改写 |
| agent 认知 | 路径不变 | 需要双向映射或全程容器路径 |
| 隔离来源 | 别人的目录**根本没挂** | 路径校验 + 挂载 |
| 工期 | 1–2 周 | 6–8 周 |

### 2.2 决策（v0.2 修订）

**采用方案 B：路径映射。**

修订理由（推翻 v0.1 的"保真优先"）：

1. **性能不构成理由**。工具调用本身以秒计（LLM 决策占大头），一次路径翻译或几十毫秒往返是噪声。
2. **抽象层 100% 覆盖**。所有 IO 一律走 `Environment` trait，`file_*` 也走 API，**不使用"宿主能直接读"的捷径**。这样换后端时工具层零改动。
3. **容器内路径更干净**。agent 看到 `/workspace`、`/skills`、`/home/pointer`，不暴露宿主目录结构，可移植性更好。
4. **保真挂载会把宿主目录树塞进容器**，边界模糊，且 `~` 展开、大小写敏感差异都会变成坑。

**唯一的性能约束是批量操作**（实测依据见 §2.4）：

| 操作 | 实现 | 往返 |
|---|---|---|
| 单文件 read/write/edit | `env.read_file()` → API | 1 |
| **grep / glob / list 大目录** | **`env.exec(["rg", ...])`** | **1** |
| terminal / read_lints / git | `env.exec()` | 1 |

**规则：批量文件操作不许逐文件调 API，必须打包成一条 exec 在沙箱内跑。**

### 2.3 性能实测（2026-11，真实 E2B envd，e2b-local + OrbStack）

数据面是 **E2B 官方 envd 二进制**，所以 exec / files 延迟为真实值。

| 操作 | 本地 `std::fs` | 经 envd API | 倍数 |
|---|---|---|---|
| exec `true` | — | **3.29 ms** | — |
| exec `echo` | — | 2.62 ms | — |
| 读 1 KB | 0.021 ms | 2.25 ms | 107× |
| 写 1 KB | — | 2.05 ms | — |
| 读 100 KB | 0.024 ms | 2.17 ms | 90× |
| 读 1 MB | 0.050 ms | 3.87 ms | 77× |
| **grep 200 文件（逐文件读）** | 5.1 ms | **603.9 ms** | **118×** |
| **grep 200 文件（一次 exec）** | — | **4.14 ms** | — |
| 沙箱创建 | — | 1353.9 ms | — |

**三条结论**：

1. **单次 exec 只要 3.3 ms**，比 `docker exec`（55.7 ms）快 17 倍——envd 是容器内常驻 HTTP 服务，没有 daemon/shim 跳转。
2. **单文件 IO 走 API 完全可以**：2–4 ms，占一次工具调用（1–3 s）的 0.1%。
3. **批量必须打包成一条 exec**：200 文件逐文件读 604 ms，一次 `grep` 4.1 ms，**差 146 倍**。1000 文件逐文件将达 ~3 s。

**对比 docker exec**：`docker exec` 55.7 ms 是纯开销（`exec true` 与 `exec cat` 同为 55 ms），而 envd 只有 3.3 ms。**这是选 E2B 协议而非 docker exec 的实证理由。**

**镜像要求（实测踩坑）**：envd 用 `/usr/bin/nice` 包装命令，**alpine 无此二进制 → 所有 exec 返回 127**。基础镜像必须含 coreutils（debian/ubuntu 可以，alpine 不行）。

**待实测**：Cube 的 exec 路径多 4 跳（CubeAPI → Master → Cubelet → Shim → envd），预估 20–80 ms；冷启动 Cube 为 60 ms，而 e2b-local+Docker 实测 1354 ms。

**验证方案：e2b-local**

CubeSandbox 官方文档（`docs/guide/usecases/oma.md`）明确：本地开发用 **e2b-local**（Go 写的 E2B 兼容网关，后端支持 Docker / OrbStack / Apple Container），云端用 CubeSandbox，**同一套接口复用调用逻辑**。

- 控制面：DTO 由 E2B OpenAPI schema 经 `oapi-codegen` 生成，非手写近似
- 数据面：直接挂 **E2B 官方 envd 二进制**，exec / files / PTY 行为与真实 E2B 一致
- 镜像：`ghcr.io/superduck-ai/e2b-local:latest`（内含 `envd-linux-{amd64,arm64}`）

可验证：协议兼容性、envd 数据面延迟、批量策略、`get_host` 语义
不可验证：Cube 控制面多跳延迟、virtiofs/Volume 语义、冷启动

**本机跑通方式（实测）**：Docker Desktop 的 `--network host` 是 VM netns，不转发到 macOS → **必须用 OrbStack**。

```bash
# 1. 取 envd 二进制（镜像内已带）
docker run --rm -v "$PWD/envd-bin:/out" --entrypoint sh \
  ghcr.io/superduck-ai/e2b-local:latest -c 'cp -r /app/envd-bin/. /out/'

# 2. 起网关（OrbStack，host 网络）
docker run -d --name e2bl --network host \
  -v ~/.orbstack/run/docker.sock:/var/run/docker.sock \
  -v "$PWD/envd-bin:$PWD/envd-bin:ro" \
  -v "$PWD/config.yaml:/config.yaml:ro" \
  --entrypoint /usr/local/bin/e2b-local \
  ghcr.io/superduck-ai/e2b-local:latest serve --config /config.yaml
```

`config.yaml`：

```yaml
traffic:
  advertised_host: "127.0.0.1"
docker:
  envd_binary: "<绝对路径>/envd-bin/envd-linux-arm64"
```

验证：`curl 127.0.0.1:3000/health` → 200，`POST /sandboxes {"templateID":"debian:bookworm-slim"}` 返回 `envdURL`。

**基准脚本**：`bench_e2b.py`（本目录）。

### 2.4 映射表

| 宿主 | 容器内 | 权限 |
|---|---|---|
| `{app_data}/session-sandboxes/{user}/` | `/home/pointer` | rw |
| `{app_data}/session-sandboxes/{conv}/` | `/workspace` | rw |
| `~/.pointer/skills/`（+ org / 内置根） | `/skills` | ro |
| `/tmp` | `/tmp` | rw |

**翻译规则**：先 `realpath` 解析符号链接，再匹配前缀；未命中一律拒绝（`PATH_NOT_MAPPED`）。工具**输出**的路径必须反向翻译，否则模型认知漂移。

---

## 3. 总体架构

```text
┌─────────────────────────────────────────┐
│ Session（会话）                          │
│  - conversation_id / session_user_id     │
│  - 绑定一个 Environment 实例             │
└───────────────┬─────────────────────────┘
                │
┌───────────────▼─────────────────────────┐
│ Environment（接口层）                     │
│  workspace_root() / skills_root()        │
│  exec() / read() / write() / list()      │
│  capabilities()                          │
└───────────────┬─────────────────────────┘
                │
     ┌──────────┼──────────┐
     ▼          ▼          ▼
  Local      Light      Container
```

**关键约束**：Environment 在会话创建时绑定，**会话中途不允许切换 Provider**。切换 = 新会话。

### 3.1 协议分层

三层各管各的，不能混成一件事：

| 层 | 协议 | 管什么 | 是否 E2B |
|---|---|---|---|
| **控制面** | E2B REST | 建沙箱、exec、文件读写、pty | ✅ |
| **GUI 通道** | VNC/RFB over WebSocket | 人看、人接管 | ❌ 自己加的一层 |
| **agent 视觉** | 无协议，读文件 | 截图给模型 | ❌ 自己定 |

```text
Pointer → Environment → CubeEnv → CubeAPI(E2B REST) → CubeProxy → MicroVM
                                                                    ├ envd
                                                                    └ GUI 容器
                                                                       ├ Xvfb+Chrome
                                                                       └ x11vnc :6080 (noVNC)
```

**E2B 协议不含 GUI。** noVNC 是叠加层，靠 `get_host(port)` 暴露端口（**待实测**）。

### 3.2 防锁死三原则

| 做 | 不做 |
|---|---|
| 抽象边界定在 `Environment` trait，IO 100% 走它 | 工具里直接 `std::fs` / `Command` |
| 自己写薄 REST 客户端（~200 行） | 绑 `cubesandbox` SDK（官方 `e2b` SDK 硬编码指向 e2b.cloud） |
| 挂载用抽象类型 `MountSpec`，provider 内部翻译 | 业务层拼 `metadata["host-mount"]` JSON |

```rust
enum MountSpec {
    UserData { tenant: TenantId },        // → Cube Volume
    ReadOnlyShared { name: String },      // → Cube Host Mount
}
```

**换后端成本**：写一个新 provider ≈ 2–3 周，工具层零改动。真正的成本是用户数据迁移（1 万用户 × 1–5 GB = 10–50 TB），与框架无关。

### 3.3 进程边界：谁在沙箱里跑

判定标准只有一条：**这件事是不是 agent 能触发的**。

- **能触发 → 一律在沙箱内执行**，遵守同一套挂载 / uid / 能力 / 命名空间规则。**不存在「宿主侧例外」**。
- **不能触发**（应用自身的系统集成）→ 留在宿主，因为它们描述的是"运行这个 app 的那台机器"，塞进沙箱反而会读到错的机器。

| 在沙箱内（agent 可触发） | 归属 | 说明 |
|---|---|---|
| 文件工具 `file_*` | `env.read_file` / `write_file` / `walk` | 已迁移 |
| `terminal` | `env.exec` / provider 流式 API | 已迁移（编排仍在 `tools/terminal_*`） |
| `read_lints`、`git` | `env.exec` | 已迁移 |
| **`media/{ffmpeg,video,audio,pdf_render}`** | `env.exec` | 处理的是租户文件，必须与 agent 同环境；**镜像需预装 ffmpeg/ffprobe** |
| **插件 `hooks` / `mcp` / `tool_provider`** | `env.exec`（常驻进程） | 插件是 agent 能力的延伸，同样受沙箱约束；需要常驻进程 API（与 terminal 同一套） |
| `computer`（截图 / 鼠标 / 键盘） | `env` 的桌面能力 | `capabilities.desktop == false` 时不下发工具（见 §6.3） |

| 留在宿主（agent 不可触发） | 原因 |
|---|---|
| `license/fingerprint` | 硬件指纹属于"运行 app 的机器"，与沙箱无关 |
| `i18n`、`unix_locale` | 读宿主 `defaults` / `locale -a`；沙箱内应改读沙箱 locale |
| `platform_auth` | 拉起浏览器做 OAuth，是宿主 UI 交互 |
| `shell_env` | 见下方"环境变量"——**沙箱内读沙箱的 PATH**，不读宿主登录 shell |

**网络：沙箱允许出网。** 联网搜索、包安装、API 调用都依赖它，默认 `NetworkPolicy::Allow`。
隔离靠 **net namespace**——沙箱之间互不可见、互不可达，**不是断网**。需要禁网的场景（例如只跑纯离线批处理）由配置显式关闭。

**环境变量分两类，不要混**：

| 类别 | 例子 | 由谁提供 |
|---|---|---|
| **沙箱自身环境** | `PATH`、`HOME`、`LANG`/`LC_*`、`PIP_USER` | `Environment`（镜像 + provider 配置） |
| **请求注入变量** | 会话级 `SESSION_USER_ID`、`POINTER_ACCESS_HOST`、`.env` 里的密钥 | 应用层在 spawn 时叠加（既有 `scoped_env_slot!`），**与 provider 无关**；凭证走同一通道（见 §8「凭证如何进沙箱」） |

也就是说：注入变量跨后端语义一致，不因为换了沙箱就变；而 `PATH`/locale 这类必须跟着环境走。

### 3.4 账号与执行环境（admin 例外）

部署默认的 provider 对**所有普通用户**生效，但 **admin 账号默认走 `local`**——因为技能维护是 admin 的职责，
而按 §5.4.1 只有 `local` 允许修改技能目录。两者是同一件事的两面，不能拆开看。

#### 解析顺序（从具体到默认）

| 优先级 | 来源 | 例 |
|---|---|---|
| 1 | **账号级显式配置** | `[environment.accounts] alice = "light"` |
| 2 | **admin 默认** | 命中 admin 名单 → **`local`** |
| 3 | **部署默认** | `[environment] provider`；缺省按 `deployment.mode` 派生 |

普通用户永远落在第 3 层。**只有名单里的人**能拿到 `local`（第 2 层），或被显式指定（第 1 层）。

#### 配置形状

```toml
# pointer-server.toml
[deployment]
mode = "platform"

[auth]
# 指定 admin 账号；值就是 session_user_id（Web 登录名 / 平台用户 id）
admins = ["admin", "wangguohong"]

[environment]
provider = "light"                       # 普通用户默认走沙箱
sandbox_url = "http://cube.internal:3000"  # provider=light 时必填

[environment.accounts]                   # 账号级覆盖（可选）
admin = "local"                          # 写出来便于审计；不写也默认 local
alice = "light"
```

环境变量等价形式：`POINTER_SERVER_ADMIN_ACCOUNTS=admin,wangguohong`、
`POINTER_SERVER_ENVIRONMENT_PROVIDER=light`、`POINTER_SERVER_ENVIRONMENT_SANDBOX_URL=…`。

#### 身份来源

复用既有 `session_user_id`（`session_user_id_env` 的 thread-local，`session_sandbox.rs` 已用它分用户目录），
**不新增身份概念**。判定入口统一为 `is_admin_account(session_user_id) -> bool`：

| 实现阶段 | 数据来源 |
|---|---|
| **先做（P2 首批）** | 配置文件名单 / 环境变量 |
| 后续 | users 表 `role` 字段（`pointer-admin` 管理） |
| 后续 | 平台控制面下发 |

换实现不影响调用方。

#### 硬规则

| 项 | 规则 |
|---|---|
| 桌面端（`standalone`） | 无账号概念，进程默认 `local`，不看名单 |
| admin 解析为 `local` | 该会话绑 `LocalEnv`（`ReadScope::Host` / `WriteScope::Host`），权限等同桌面端 |
| admin 想走沙箱 | `[environment.accounts] admin = "light"` 显式覆盖 |
| `provider = "light"` 但缺 `sandbox_url` | **启动即报错**（该账号解析为 light 时同样报错），不静默退回 `local` |
| 非名单用户配 `accounts = "local"` | 允许（调试），但启动 WARN + 会话审计 |
| 粒度 | **进程级 + 账号级**，不是每会话切换；会话只决定绑哪个沙箱实例 |

#### 两道必须补的保护

1. **审计**：light 部署下出现 `provider=local` 的会话要记一条（谁 / 何时 / 哪个会话）——它意味着 agent 能碰宿主文件系统。
2. **UI 明示**：admin 的 local 会话在界面上标注「此会话运行在宿主环境」，避免误以为在沙箱内。

#### 已知冲突

多个 admin 同时用 `local` → 写的是**同一个** `~/.pointer/skills/`（local 下 skills 不按用户分目录）。
同时改同一技能会互相覆盖。先做「写入串行化」或至少启动提示；admin 通常一两人，暂不引入锁文件。

---

## 4. 阶段一：基础框架 + local 模式

### 4.1 目标

- 引入 `Environment` 抽象，但只实现 local
- 现有行为零回归
- 为后续两档预留接口

### 4.2 接口

```rust
pub trait Environment: Send + Sync {
    fn kind(&self) -> EnvKind;

    /// 提示词注入用。local 返回宿主真实路径。
    fn workspace_root(&self) -> &Path;
    fn skills_root(&self) -> &Path;
    fn home_root(&self) -> &Path;

    fn capabilities(&self) -> Capabilities;

    fn exec(&self, argv: &[String], opts: ExecOptions) -> Result<ExecOutput>;
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;
    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()>;
    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;
}

pub struct Capabilities {
    pub desktop: bool,
    pub read_scope: ReadScope,   // Host | User | Workspace | None
    pub write_scope: WriteScope,
    pub snapshot: bool,
    pub network: NetworkPolicy,  // 默认 Allow：沙箱允许出网，见 §3.3
}
```

### 4.3 改造点

| 位置 | 改动 |
|---|---|
| `agents/mod.rs:513` 附近 | `vars.workspace_root` 从 Environment 取，不再从 settings 直读 |
| `tools/file/path.rs` | 5 个 resolver 增加 `&Environment` 参数，local 分支恒等 |
| `tools/file/*.rs` | IO 调用改为 `env.read_file` / `env.write_file` |
| terminal 工具 | `Command` 改为 `env.exec` |
| `session_sandbox.rs` | 保持现状，作为 local 的目录来源 |

### 4.4 验收

- [ ] `cargo test -p pointer-core` 全绿
- [ ] 现有会话行为与改造前逐字节一致（同 prompt、同文件读写结果）
- [ ] `grep -rn 'std::fs' crates/pointer-core/src/tools/` **归零**（只允许出现在 Environment 实现内部）
- [ ] `grep -rn 'Command::new' crates/pointer-core/src/tools/` **归零**（exec 全走 trait）
- [ ] 全仓搜不到硬编码的 `session-sandboxes` 拼接（统一走 `session_sandbox.rs`）

### 4.5 风险

| 风险 | 缓解 |
|---|---|
| 抽象泄漏（某工具拿不到 env） | 编译期强制：工具构造函数必须接收 `Arc<dyn Environment>` |
| 子 agent 继承 | `run_subagent` 传递同一个 env 句柄 |
| worktree 场景 | 阶段一保持本地实现，worktree 只在 local 下可用 |

---

## 5. 阶段二：light 模式（多用户隔离，无 GUI）

### 5.1 目标

Web 端多用户并发，满足：

1. 用户 A 的 agent **看不见** 用户 B 的任何文件
2. skills 目录对所有用户**只读**
3. 不需要桌面环境

### 5.2 隔离模型

四层叠加，缺一不可：

| 层 | 机制 | 挡住什么 |
|---|---|---|
| **挂载** | 只挂当前用户 + 当前会话 + skills | 别的用户目录不存在 |
| **身份** | 每用户独立 uid（100000 起，永不复用） | 路径可达时的 DAC 拒绝 |
| **能力** | `--cap-drop ALL` + `no-new-privileges` | remount / setuid 提权 |
| **命名空间** | 独立 mount / pid / net / ipc | 进程与网络互不可见 |

> **注意**：独立 net namespace 指的是**沙箱之间互不可见**，不是断网。沙箱默认允许出网（见 §3.3）。

### 5.3 目录映射（路径映射）

| 宿主路径 | 容器内 | 权限 | Cube 挂载方式 |
|---|---|---|---|
| `{app_data}/session-sandboxes/{user}/` | `/home/pointer` | `rw` | Volume |
| `{app_data}/session-sandboxes/{conv}/` | `/workspace` | `rw` | Volume |
| `~/.pointer/skills/`（+ org / `~/.agents/skills` / 内置根） | `/skills/<root>` | `ro` | Host Mount（`readOnly: true`） |
| `/tmp` | `/tmp` | `rw` | tmpfs |

agent 只看到 `/workspace`、`/home/pointer`、`/skills`，不暴露宿主目录结构。

### 5.4 启动方式（Cube）

**关键约束：用户数据必须走 Volume / Host Mount，不能放 rootfs。**

rootfs 是 overlay（lower 只读 + upper 可写），宿主看到的是分离的两层，合并视图只存在于 guest 内；而 Volume / Host Mount 的底层就是宿主目录，宿主可以直接 `std::fs` 读写——这正是路径映射只需要"字符串翻译"的原因。

```python
from cubesandbox import Sandbox, Volume
import json

vol = Volume.create(f"ws-{tenant_id}")          # 每用户一个 Volume

mounts = json.dumps([
    {"hostPath": "/home/you/.pointer/skills", "mountPath": "/skills/user", "readOnly": True},
])

with Sandbox.create(
    template=TEMPLATE_ID,
    volume_mounts={"/workspace": vol, "/home/pointer": vol},
    metadata={"host-mount": mounts},
    env={"HOME": "/home/pointer", "PIP_USER": "1",
         "PYTHONUSERBASE": "/home/pointer/.local"},
) as sb:
    ...
```

**必须配置的两个点**：

| 项 | 值 | 原因 |
|---|---|---|
| virtiofs `cache` | **`never`** | `always` 会让宿主写入对 guest 不立即可见 |
| `host-mount` 前缀白名单 | 仅技能根与 `session-sandboxes` | 默认只允许 `/data/shared/` 下路径，需收紧 |

### 5.4.1 skills 只读的双份保护

`readOnly: true` 只把 guest 侧的 bind mount remount 成 `MS_RDONLY`，**底层共享目录仍可被宿主写**。所以：

| 位置 | 机制 |
|---|---|
| guest 内 | `readOnly: true` + `MS_NOSUID/NOEXEC/NODEV` |
| 宿主进程 | `path.rs` 对 `skills_root` 显式拒绝写（仅 light / container 生效） |

**生效范围（已定）**：**只有 `local` 允许修改技能目录**——桌面端 coder 改技能是主要工作流，
必须保留写权限；`light` / `container` 一律只读，且**不提供 per-user 可写副本**（见 §8 第 2 条）。
Web 端的技能维护由 **admin 会话**承担：admin 默认解析为 `local`（§3.4），普通用户始终在 `light` 里只读。
所以在 local 档位下不施加宿主侧拒绝，是设计选择而非遗漏。

### 5.5 skills 挂载：直接只读挂载

**结论：不做发布流水线。** `~/.pointer/skills`（以及 org / `~/.agents/skills` / 内置根）**直接以只读方式挂进沙箱**，
`/skills` 就是这些根——不需要 rsync 白名单、不需要 `@<ver>` 版本目录。

前提只有两条，都是**约定**而不是流水线：

| 前提 | 说明 |
|---|---|
| 技能目录**不放用户数据** | 运行时产物（中间结果、缓存、日志、导出件）一律写 `$WORKING_DIR`；平台已通过 `session_work_dir_env` 注入该变量 |
| 技能目录**不放个人凭证** | `.env` **允许**（技能脚本运行需要，属技能自身环境配置）；裸凭证文件（`token.txt` / `*_token.txt`）不放；**个人**凭证按 §8 走请求级 env 注入 |

**挂载前唯一的前置检查：凭证扫描**（命中即拒绝启动，不做静默剔除）：

```bash
# 命中就报错，不"过滤掉继续跑"（`.env` 不算：它是技能脚本需要的环境配置）
find ~/.pointer/skills -maxdepth 3 \
  \( -name 'token.txt' -o -name '*_token.txt' \) -print
```

为什么是拒绝而不是剔除：静默剔除会让人误以为"凭证放技能里也没事"，下次换个文件名就漏了；
凭证本来就该走 §8 的注入通道，出现在技能目录里就是配置错误。

**多个技能根怎么挂**：每个根各自 `readOnly` 挂到 `/skills/<root-name>/`，用既有 env 机制
（`SKILL_DIR` 等，见 `skills/external.rs::apply_skill_dir_env`）告诉沙箱内的加载器；
`{baseDir}` 在沙箱内解析到对应挂载点。

**运行时产物往哪写**：`{baseDir}` 只视为**只读代码**；一切产物写 `$WORKING_DIR`。

| 谁负责 | 内容 |
|---|---|
| 平台 | 注入 `WORKING_DIR`；`/skills` 只读；挂载前扫凭证 |
| 技能作者 | 产物路径一律 `$WORKING_DIR/...`；`{baseDir}` 只用于读代码/资源；不放凭证、不放用户数据 |

**版本 / 回滚：不做。** 技能更新就地生效（下次会话看到新版本）。将来要灰度或回滚再加 `@<ver>`；
当前 11 个技能、一两个 admin，不值得为它引入一套发布物。

**overlayfs 不作为当前方案**（lower=只读代码，upper=每用户可写）：§8 第 2 条已定 `light` / `container`
下 skills 一律只读，只有 `local` 可改。仅当将来要放开「Web 端用户自助改技能」时再评估——多一层文件系统
就多一处要验证的语义。

### 5.6 验收（可执行）

```bash
# 1. 跨用户不可见
sandbox exec <id> -- ls /home/pointer/../../{other_tenant}   # 期望：No such file or directory

# 2. 路径逃逸无效
sandbox exec <id> -- cat /workspace/../../etc/passwd   # 期望：不存在

# 3. skills 只读
sandbox exec <id> -- touch /skills/x                   # 期望：Read-only file system

# 4. 无法提权
sandbox exec <id> -- mount -o remount,rw /skills       # 期望：Operation not permitted

# 5. 出网可用（联网搜索 / 装包依赖它；隔离靠 net namespace，不是断网）
sandbox exec <id> -- curl -sS -o /dev/null -w '%{http_code}' https://example.com   # 期望：200

# 6. 凭证不可见
sandbox exec <id> -- find /skills \( -name 'token.txt' -o -name '*_token.txt' \)   # 期望：空（`.env` 允许）

# 7. 插件 / 媒体进程也在沙箱内（不是宿主 uid）
sandbox exec <id> -- id            # 期望：uid ≥ 100000，且不是宿主用户
sandbox exec <id> -- ps -o user= -p $$   # 期望：同上
```

### 5.7 容量

| 项 | 值 |
|---|---|
| 单实例 | 1–2 vCPU / 1–2 GB |
| 目标并发 | 10–20（1 万注册用户，低并发） |
| 冷启动 | 60 ms（Cube 快照克隆） |
| 每用户配额 | 1–5 GB |

### 5.8 风险

| 风险 | 缓解 |
|---|---|
| ~~共享内核，内核 0day 跨用户~~ | **已由 Cube 解决**：每沙箱独立 Guest 内核 |
| 用户把 workspace 塞满 | 宿主侧 quota，超限拒绝写 |
| 技能写 `{baseDir}` 冲突 | 技能产物统一落 `$WORKING_DIR`（§5.5），`/skills` 只读 |
| 大小写敏感差异（macOS 宿主 vs Linux 容器） | 挂载前统一 `realpath`，测试覆盖 |
| 单机单点 | 卷异地备份；沙箱层可在另一台重建 |

---

## 6. 阶段三：容器模式（含 GUI）

### 6.0 运行时选择：Cube 优先

| 项 | 选择 |
|---|---|
| 默认 | **Cube**（与 light 同一套运行时） |
| 备选 | Docker，仅当需要 GPU 直通或复用现成镜像（`linuxserver/webtop`、E2B Desktop） |

理由：

1. **GUI 才是高风险面**——浏览器里有登录态、截图含敏感内容。把 GUI 放共享内核的 Docker、把纯文本任务放内核隔离的 Cube，是安全强度与风险倒挂。
2. **两套 runtime = 双倍运维**：部署、监控、镜像管理、路径映射实现都要两套。
3. **GUI 不需要额外能力**：OCI 模板里装 Xvfb + WM + 浏览器即可，Cube 的限制（无 GPU）Docker 同样有（不直通也是 llvmpipe）。

**前置验证（进入本阶段前必须做）**：Cube 能否把 guest 内的 noVNC 端口暴露给外部。E2B 协议有 `get_host(port)`，Cube 声称 drop-in 兼容，但需实测。不通则回退 Docker。

### 6.1 与 light 的差异

| 维度 | light | container |
|---|---|---|
| 桌面 | 无 | Xvfb + WM + 浏览器 |
| 资源 | 1–2 GB | 4–8 GB |
| 能力 | exec / file | + screenshot / mouse / keyboard |
| 工具集 | 全量文本工具 | 按 `capabilities.desktop` 动态裁剪 |

### 6.2 新增接口

```rust
fn display_info(&self) -> Option<DisplayInfo>;
fn screenshot(&self, fmt: ImageFormat) -> Result<Vec<u8>>;
fn list_windows(&self) -> Result<Vec<WindowInfo>>;
fn mouse_move(&self, x: i32, y: i32) -> Result<()>;
fn click(&self, x: i32, y: i32, button: MouseButton) -> Result<()>;
fn type_text(&self, text: &str) -> Result<()>;
fn key_press(&self, combo: &str) -> Result<()>;
```

### 6.3 工具集联动

`capabilities.desktop == false` 时，`screenshot` / `click` 等工具**不出现在模型可见的工具列表里**，而不是调用时返回错误。

### 6.4 固定参数

```bash
--shm-size=512m
-e DISPLAY=:99
# 浏览器
--window-size=1920,1080
--use-gl=angle --use-angle=swiftshader --enable-unsafe-swiftshader
--disable-gpu-compositing --password-store=basic
```

### 6.5 验收

- [ ] GUI 任务在沙箱内跑通，宿主鼠标键盘零干扰
- [ ] `nproc` / `free` 返回配额而非宿主规格（资源视图隔离）
- [ ] 截图与点击坐标一致（分辨率 / DPI 固定）
- [ ] 人工接管：noVNC 能从浏览器打开并操作

### 6.6 镜像分层与预装

**决策：共享镜像预装基础环境，用户不需要自己装。**

Cube 的 template 是快照克隆（CoW），镜像 1.2 GB 还是 3 GB，冷启动都是 60 ms——克隆不复制数据。所以 fat image 在 Cube 上几乎不付代价，只占一份存储。

| 镜像 | 内容 | 体积 |
|---|---|---|
| `agent-base` | debian-slim + 中文 locale/字体 + Python 3.12 + Node 22 + git + 常用 CLI + gcc/make + imagemagick | ~1.2 GB |
| `agent-data` | base + numpy/pandas/matplotlib/scipy/openpyxl | ~1.8 GB |
| `agent-office` | data + LibreOffice + pandoc | ~3 GB |
| `agent-gui` | base + Chrome + Xvfb + openbox + noVNC | ~1.5 GB |

**base 层清单**：`locales` + `zh_CN.UTF-8`、`fonts-noto-cjk`、`tzdata`（`Asia/Shanghai`）、python3-pip/venv、nodejs/npm、`gcc g++ make`、`imagemagick`、`poppler-utils`、**`ffmpeg`（含 ffprobe，媒体工具与视频处理依赖它）**、`git curl wget jq ripgrep unzip tar`、`dbus`。

**锁版本，不用 latest**，否则不可复现。

**预装放镜像层，用户装的放 Volume**：

```bash
-e PIP_USER=1
-e PYTHONUSERBASE=/home/pointer/.local
-e NPM_CONFIG_PREFIX=/home/pointer/.npm-global
-e PATH=/home/pointer/.local/bin:/home/pointer/.npm-global/bin:$PATH
```

| 行为 | 落点 | 是否持久 |
|---|---|---|
| 镜像预装 | 镜像层（只读） | 跨会话、跨用户共享 |
| `pip install --user` / `npm i -g` | Volume | ✅ 持久 |
| `apt install` | sandbox 可写层 | ❌ 销毁即弃 |

**注意**：`apt install` 不持久，提示词里要引导 agent 优先用 `pip`/`npm`。

**每用户不需要独立镜像**：1 万用户 × 1.2 GB = 12 TB，且平台升级要重建 1 万份。共享镜像 + 每用户 Volume 即可。

---

### 6.7 容器档位落地记录（2026-11-15）

docker 后端同时承载 light 与 container 两个档位，差别只在镜像与能力集。

| 项 | light | container |
|---|---|---|
| 镜像 | `pointer-sandbox-light:latest` | `pointer-sandbox-container:latest` |
| 桌面 | 无 | Xvfb `:99` 1920×1080 + openbox |
| `Capabilities` | `light()`（无桌面/无快照） | `container()`（桌面 + 快照） |
| 桌面工具 | — | `xdotool` / `xwininfo` / `import`(ImageMagick) / `scrot` / `x11vnc` |
| 工具链 | Node 24 + Python 3.12 + ffmpeg + poppler | 同上 |

关键实现：

1. **入口脚本**（`docker/entrypoint-container.sh`）：起 Xvfb → 轮询 `xdpyinfo` 等就绪 → 起 openbox → 执行 CMD。
   openbox 用 `DISPLAY=` 环境变量传显示器（它不接受 `--display` 参数）。
2. **就绪等待**：`DockerEnv::create` 在 container 档位下轮询 `docker exec xdpyinfo`，最多 10s。
   不等就会拿到 `cannot open display`——Xvfb 是异步起的。
3. **DISPLAY 注入**：`exec` / `spawn` 时自动补 `DISPLAY=:99`，调用方不用手写。
4. **规格指纹**：容器打 `pointer.spec=<hash>` 标签（**契约版本** + 镜像 + 档位 + 挂载 + 工作目录）。
   契约版本 `SPEC_VERSION` 改了入口脚本/注入的环境变量/挂载语义就 +1——
   否则旧容器会被继续复用，表现为"改了配置没生效"（踩过：加了 VNC 密码变量，
   旧容器没重建，连上去仍然无密码）。
   复用前比对，不一致就 `rm -f` 重建——否则改配置后仍拿旧镜像的容器（"改了没生效"）。
5. **资源限制**：`DockerSpec.cpus` / `memory_mb` → `--cpus` / `--memory`。
6. **桌面控制后端**（`agents/computer/control/docker.rs`）：实现 `ComputerControl` 全套方法，
   鼠标/键盘走容器内 `xdotool`，截图走 `import -window root`。
   按会话选择：`control_for_conversation()` → `Container` 用 `DockerControl`，其余用宿主 `LocalControl`。
7. **容器剪贴板**：`ComputerControl` 增加 `set/get_clipboard_text`，容器后端用 `xclip`。
   宿主剪贴板与容器 X 剪贴板是两个选择区，粘贴必须跟着后端走，否则粘出来是空的。
8. **预装 GUI 应用**：`chromium`（包装成 `browser`，带 `--no-sandbox`）、`xterm`、`xclip`。
9. **VNC 随机密码**：每个容器生成 8 位随机密码（去掉 `0/O/1/l/i` 易混字符），
   通过 `POINTER_VNC_PASSWORD` 传给容器，入口用 `x11vnc -rfbauth`。
   查法：`scripts/desktop-view.sh` 会打印并复制到剪贴板。
10. **磁盘限制**：`DockerSpec.disk_mb` → `--storage-opt size=`。
   只有 Linux + overlay2/xfs(pquota) 支持；macOS 会报错，此时**退回不限磁盘并告警一次**，
   不能让"磁盘限制"把沙箱整个建不起来（`--memory` / `--cpus` 不受影响）。

验收（本机）：

```
docker run --rm pointer-sandbox-container:latest sh -lc 'chromium --version'
  → Chromium 156.0.8078.79
容器内 `browser about:blank` → 截图可见浏览器窗口（标题栏 about:blank）
xclip 写入后读回：hello-clipboard
```

端到端验收（`tests/container_tier_it.rs`）：

| 用例 | 断言 |
|---|---|
| `gui_task_navigates_and_screenshot_changes` | 点地址栏 → 输入 data URL → 回车 → 红色像素占比 >20%，且前后截图不同 |
| `container_has_gui_apps_and_clipboard` | chromium/xterm/xclip/browser 都在；chromium 窗口存在；剪贴板能写能读 |
| `docker_control_drives_the_container_desktop` | 屏幕 1920×1080；截图是 JPEG；移动后光标位置读回一致 |
| `capture_screen_reads_the_container_desktop` | 容器桌面刷红后取帧，红色像素占比 >20%，帧尺寸 = 容器虚拟屏 |

**截图链路也必须走环境**（`state::capture_screen`）：容器档位下取帧改走
`capture_container_frame()`（`DockerControl` 的 `xdotool`/`import`），
`screen_bbox` 用容器虚拟屏尺寸，不再用宿主显示器信息。

否则会出现"宿主截图 + 容器点击"：坐标数值能对上、画面内容对不上，点了不报错但点错位置——
这种静默错最难查。

> **快照**：`Capabilities::container().snapshot` 目前是 `false`。快照尚未实现，
> 能力位不能写 `true`——声明了却做不到，上层会误以为"跑完能回滚"。

验收（本机）：

```
docker run --rm pointer-sandbox-container:latest sh -lc 'node -v; python3 -V; xdotool getdisplaygeometry'
  → v24.22.3 / Python 3.12.16 / 1920 1080
```

## 7. 里程碑

| 阶段 | 内容 | 工期 | 关键交付 |
|---|---|---|---|
| **P1** | Environment 抽象 + local + 路径翻译（正向/反向） | 2 周 | 零回归，IO 全部收敛到 trait（`grep 'std::fs' tools/` 归零） |
| **P2** | light：Cube 接入 + Volume/Host Mount + 文件 API + 批量走 exec | 3–4 周 | 六条验收命令全过 |
| **P3** | 生命周期 + 配额 + 审计 + 镜像分层 | 1 周 | 1 万用户 / 20 并发压测 |
| **P4** | container + GUI（先验证 Cube 端口暴露） | 2–3 周 | computer 闭环 + noVNC 接管 |

### 7.1 实现顺序（P1 起步）

**原则：先立骨架，再逐个工具迁移。每迁一个跑一次测试，不一次性改完。**

**第一个 PR（建议 3–5 天）**：只做骨架，不动任何工具。

| # | 动作 | 文件 |
|---|---|---|
| 1 | 定义 trait 与数据结构 | `crates/pointer-core/src/env/mod.rs`（新建）<br>`Environment` / `Capabilities` / `ExecOptions` / `ExecOutput` / `MountSpec` |
| 2 | 实现 `LocalEnv` | `crates/pointer-core/src/env/local.rs`（新建）<br>路径恒等，IO 走 `std::fs`，exec 走 `Command` |
| 3 | 路径翻译器 | `crates/pointer-core/src/env/pathmap.rs`（新建）<br>正向 `to_host` / 反向 `to_agent`，local 下为恒等 |
| 4 | 接到现有解析层 | `crates/pointer-core/src/tools/file/path.rs`<br>5 个 resolver 增加 `&dyn Environment` 参数，**保持现有行为不变** |
| 5 | 提示词注入改来源 | `crates/pointer-core/src/agents/mod.rs:513`<br>`vars.workspace_root` 改从 `env.workspace_root()` 取 |
| 6 | 单测 | `LocalEnv` 往返翻译、边界拒绝、`MountSpec` 映射 |

**第二个 PR：迁移工具（按依赖顺序，一个工具一个 commit）**

```text
file_read   → file_write → file_edit → file_list → file_glob → file_grep
```

- `file_grep` / `file_glob` **必须**改成沙箱内一次 `exec`（实测 142 倍差距，见 §2.3）
- 每迁完一个，跑该工具的现有单测，确保行为不变

**第三个 PR：exec 类**

```text
terminal → read_lints → git status/diff
```

`read_lints` 最麻烦——现在本地跑编译器再解析输出，要改成 `env.exec` + 解析 JSON 里的路径并反向翻译。

**验收命令（每个 PR 都要过）**

```bash
cargo test -p pointer-core
grep -rn 'std::fs' crates/pointer-core/src/tools/     # 目标：归零
grep -rn 'Command::new' crates/pointer-core/src/tools/ # 目标：归零
```

**对沙箱跑集成测试**（P2 起）：

```bash
scripts/dev-sandbox.sh up
cargo test -p pointer-core --features sandbox-it   # 或独立的集成测试包
scripts/dev-sandbox.sh down
```

集成测试要写成「只换 base URL 就能跑两套后端」，拿到真 Cube 时零改动切换。

### 7.2 实现记录（P1 第一刀，已落地）

| 交付 | 文件 | 状态 |
|---|---|---|
| `Environment` trait + `Capabilities` / `ExecOptions` / `ExecOutput` / `DirEntry` / `EnvKind` / `ReadScope` / `WriteScope` / `NetworkPolicy` | `crates/pointer-core/src/env/mod.rs` | ✅ |
| `LocalEnv`（路径恒等、`std::fs`、`Command`、可选超时） | `crates/pointer-core/src/env/local.rs` | ✅ |
| `PathMap` + `MountSpec`（正/反向翻译、`PATH_NOT_MAPPED`、只读判定） | `crates/pointer-core/src/env/pathmap.rs` | ✅ |
| 模块注册 | `crates/pointer-core/src/lib.rs` | ✅ |
| 提示词 `{{workspace_root}}` 改由环境提供 | `chat_service/single_agent_prompt.rs`、`chat_service/sub_agent_prompt.rs`（3 处构造） | ✅ |
| 路径层接入环境：工作区来源 + 读写范围收口 | `tools/file/path.rs` | ✅ |

**与 §7.1 的偏差（有意为之）**：

1. **env 句柄用 thread-local，而不是给 resolver 加 `&dyn Environment` 参数。**
   `ToolHandler` 的类型是 `Arc<dyn Fn(Value) -> Result<String>>`，注册只在启动时发生一次，
   无法按会话注入参数。改用与 `scoped_env_slot!`（`SESSION_USER_ID` / `WORKING_DIR`）同一套
   `EnvironmentGuard`：绑定在分发点重放，`env::current_env()` 读取。
   好处是 4 个公开 resolver 的签名与 10 个调用点零改动，回归面最小。
2. **§4.3「5 个 resolver」实际是 4 个公开入口 + `writable_path_roots`。**
   `resolve_relative_under_workspace` 是私有函数，只有一个内部调用者。
3. **skills 只读暂不在宿主侧拒绝。** 已定「只有 `local` 允许修改技能」——桌面端 coder 改
   `~/.pointer/skills/` 是主要工作流，所以 local 不施加拒绝；`light` / `container` 的只读保护
   （§5.4.1 双份保护）随 P2 provider 一起生效。

**第二个 PR（工具 IO 迁移）与第三个 PR（exec 类）—— 已落地**：

| 交付 | 文件 | 状态 |
|---|---|---|
| `Environment` 补齐 IO 能力：`metadata` / `exists` / `canonicalize` / `create_dir_all` / `read_to_string` / `open_reader` / `walk` / `remove_file` / `remove_dir_all` / `set_executable` | `env/mod.rs`、`env/local.rs` | ✅ |
| 进程后端：`command` / `configure_process_group` / `kill_tree` / `kill_group` / `suppress_console_window` | `env/process.rs`（新增） | ✅ |
| `file_read` / `file_write` / `file_edit` / `file_list` 全部 IO 走 env（`open_reader` 保证大文件仍按行流式扫描） | `tools/file/{read,write,edit,list}.rs` | ✅ |
| `file_glob` / `file_grep` 改**一次批量遍历** `env.walk`（不再逐目录往返），内容读取走 `env.open_reader` + `search_reader` | `tools/file/{glob,grep}.rs` | ✅ |
| 路径提示（`collect_path_hints`）改 `env.list_dir` | `tools/file/path.rs` | ✅ |
| terminal 的 spawn / 进程组 / kill 树下沉到 `env/process.rs`；elevated / askpass 的 `fs` 与 `Command` 改走 env | `tools/terminal.rs`、`tools/terminal_elevated.rs`、`tools/terminal_askpass.rs` | ✅ |
| `read_lints` 两个 runner 改 `env.exec`（超时 + 每流输出上限 + 无退出码语义） | `agents/coder/read_lints/exec.rs` | ✅ |
| `git`（worktree 与 workspace_read 两条路径）改 `env.exec`；`ExecOptions` 新增 `env_remove` 保留 `GIT_DIR` 等加固剔除 | `worktree/git.rs`、`workspace_read.rs` | ✅ |
| 图片读取走 `env.read_file` | `tools/media_understand/dispatch.rs` | ✅ |

**验收结果**（`crates/pointer-core/src` 下）：

- `grep -rn 'Command::new' tools/` → **仅剩 1 条测试代码**（`terminal_elevated.rs` 的 `#[cfg(test)]` 里探测 `git --version`）。
- `grep -rn 'std::fs' tools/` → **生产代码归零**，剩余命中全部在 `#[cfg(test)]` 模块（`file/mod.rs`、`parallel.rs`、`path.rs` 测试、`terminal.rs` 进程树测试）。
- `cargo test -p pointer-core --lib` → **2218 passed, 0 failed, 10 ignored**。

**仍留的债（P2 前不阻塞）**：

1. `tools/file/path.rs` 的**路径校验谓词**（`Path::is_dir` / `exists` / `canonicalize`）仍直接打宿主 FS —— 它们是纯本地校验，
   改签名会波及 10 个调用点，随 P2 路径翻译一起做。
2. terminal 的 **PTY / 提权 / askpass 编排**仍在 `tools/terminal_*`：这几条路径依赖交互式 UI 回调，
   需要 provider 级流式 API（`env.exec_streaming`）才能整体下沉，`env/process.rs` 是那个替换点。
3. **其余 `Command::new` 按 §3.3 的边界分类处理**——它们不是「宿主例外」，只是本轮没迁移：

   | 站点 | 归属 | 何时做 |
   |---|---|---|
   | `media/{ffmpeg,video,audio,pdf_render}` | **沙箱内**（租户文件处理） | ✅ 已改走 `env.exec`（`media/exec.rs`：`run` + `StagedFile`）；镜像需预装 ffmpeg/ffprobe |
   | 媒体输入**路径化** | 大文件不整段进宿主内存 | ✅ `MediaInput::{Existing,Staged}`；≥32 MiB 只传路径；单帧产物上限 6 MiB；非普通文件（FIFO/设备）直接拒绝；`read_file_capped` 兜底 |
   | `plugins/{mcp,tool_provider}` | **沙箱内**（插件是 agent 能力延伸） | ✅ 已改走 `Environment::spawn`（常驻进程 + `ChildHandle`）；local 实现完成，remote 明确报错（双向流待做） |
   | `plugins/hooks` | **沙箱内** | ⏳ tokio 异步子进程，light 下先**显式拒绝**（不落宿主），随 remote spawn 一起做 |
   | `platform/app_access/*` | **沙箱内**（computer 工具） | ✅ 已接 `capabilities.desktop` 门控（light 下明确拒绝，不落宿主）；container 阶段再做 provider 版 |
   | `shell_env`（`terminal.rs:348` 探宿主 PATH） | 沙箱内环境的 `PATH` | P2：PATH 由 `Environment` 提供；请求注入变量仍走应用层 |
   | `license/fingerprint`、`i18n`、`platform_auth`、`unix_locale`、`client_env` | **宿主保留** | 不进 Environment（描述运行 app 的机器） |



### 7.2.1 light 模式（P2 第一刀，已落地）

| 交付 | 文件 | 状态 |
|---|---|---|
| `RemoteEnv`：薄 REST 客户端（不引 e2b SDK），files API + Connect 帧执行 + find/rg 批量遍历 | `env/remote.rs`（新增） | ✅ |
| provider 三层解析（账号覆盖 → admin 默认 local → 部署默认）+ light 缺 `sandbox_url` 即报错 | `env/provider.rs`（新增） | ✅ |
| 挂载表：`/home/pointer`、`/workspace`（按会话隔离）、`/skills/<name>`（ro） | `env/mounts.rs`（新增） | ✅ |
| 会话绑定：按会话缓存沙箱 + 30min 空闲回收 + `release` / `release_all`；挂载前凭证扫描 | `env/session_env.rs`（新增） | ✅ |
| `[environment]` / `[auth] admins` 配置段 + 4 个访问器；`POINTER_SKILLS_DIR` 可覆盖技能根 | `server_config.rs`、`skills/external.rs` | ✅ |
| 三处 thread-local 重放（light 下工作区根改 guest `/workspace`） | `chat_service/session_inner.rs`、`dispatch/registry.rs`、`chat_service/sub_agent.rs` | ✅ |
| 会话删除时回收远端沙箱 | `conversation_store/mod.rs`（2 个删除钩子） | ✅ |
| §5.6 七条验收脚本（含后端能力探测，缺能力标 SKIP 而非 PASS） | `scripts/verify-sandbox-isolation.py`（新增） | ✅ |
| `RemoteEnv` 双挂载后端：有 `volume_host_path` → `volumeMounts`（含 `readOnly`）；无 → Cube 的 `metadata["host-mount"]` | `env/remote.rs` | ✅ |
| 常驻进程接口：`Environment::spawn` + `ChildHandle`（write/close_stdin/reader/try_wait/wait/kill） | `env/mod.rs`、`env/local.rs` | ✅ |
| `RemoteEnv::spawn`：Start 后台 + 后台泵抽 stdout + `SendInput`/`CloseStdin`/`SendSignal` | `env/remote.rs` | ✅ e2b-local 实测通过 |
| 插件 hooks 迁到 `Environment::spawn`（写 stdin → 关 stdin → 读 stdout，超时显式 kill） | `plugins/hooks.rs` | ✅ |
| 用户插件根只读挂载 `/plugins` + hook 脚本路径宿主→沙箱映射 | `env/mounts.rs`、`env/session_env.rs`、`plugins/hooks.rs` | ✅ e2b-local 实测通过 |
| 沙箱镜像 `docker/sandbox-light.Dockerfile`（ffmpeg / poppler / 中文字体 / rg / python / node） | `docker/`、`[environment] template` | ✅ |
| 路径判定环境感知（light 下 `/workspace` 不再被宿主 `is_dir()` 误判） | `tools/file/path.rs` | ✅ |
| `[environment] volume_host_path` 配置 + 网关侧 `docker.volume_host_path` 与 `~/.e2b-local` 挂载 | `server_config.rs`、`scripts/dev-sandbox.sh` | ✅ |

**e2b-local 实测到的协议事实**（写进 `env/remote.rs` 头注释）：

1. 建沙箱返回 **201**（不是 200），`{sandboxID, envdURL}`，创建到可执行约 1.2s；
2. 文件 IO 走 `GET|POST /files?path=&username=root` 原始字节；**目录列举没有 REST 接口**（返回 400 `is a directory`）→ 列举一律走 exec；
3. 执行走 `POST /process.Process/Start` + Connect 帧；`data.stdout/stderr` 是 **base64**，`end.exitCode` 在退出码为 0 时**缺省**，`flags & 0x02` 是流结束；
4. **envd 不认 `ProcessStartRequest.timeout`**（实测 `sleep 5` + `timeout:1` 仍跑满 5s）→ 改用沙箱内 coreutils `timeout -s KILL` 兜底，退出码 124/137 映射为 `timed_out`；
5. **挂载要分后端看**（2026-11-15 实测 + 官方文档核对）：
   - `metadata["host-mount"]` 是 **Cube 语义**，e2b-local 直接忽略；
   - e2b-local 用 **`volumeMounts` + `docker.volume_host_path`**：卷目录必须落在**宿主可见路径**
     （默认是网关容器内 `/root/.e2b-local/volumes`，docker daemon 看不到 → 必 500）。
     配好之后实测 `/workspace` 为 `type virtiofs (rw)`，**沙箱写 → 宿主可见、宿主写 → 沙箱可见**，双向通。
     `scripts/dev-sandbox.sh` 已补 `volume_host_path` 与同路径 `-v` 挂载。
   - **只读语义**：E2B 官方明确「API 与 SDK 不支持只读挂载」；Cube 用扩展
     `volumeMounts[].readOnly: true`；**e2b-local 忽略该字段**（实测 `readOnly:true` 仍是 `rw`，`touch` 成功）。
     所以 `/skills` 只读只能在 Cube 上验收，隔离脚本按实际挂载标志判 SKIP 而不是 PASS。
   - **e2b-local 卷的三条硬约束**（踩过才知道）：
     1. 卷目录必须在 `docker.volume_host_path` 下，且该路径要挂进网关容器（否则 daemon 看不到 → 500）；
     2. 卷必须经 `POST /volumes` 注册，**自己预建目录会被拒**
        （`refusing to use unmanaged docker local volume directory`）；
     3. **符号链接不被接受**（`is not a directory`），只能是真实目录。
     因此挂载点采用**扁平命名**（`{user}-home`、`{user}-ws-{conv}`），目录名即卷名。

**验收（2026-11-15）**

| 命令 | 结果 |
|---|---|
| `cargo test -p pointer-core --lib` | **2268 passed**, 0 failed |
| `cargo test -p pointer-core --test tool_dispatch_smoke` | 1 passed |
| `POINTER_SANDBOX_URL=… cargo test -p pointer-core --test remote_env_it -- --ignored` | 1 passed（2.7s） |
| `POINTER_SANDBOX_URL=… cargo test -p pointer-core --test light_binding_it -- --ignored` | 1 passed |
| `python3 scripts/verify-sandbox-isolation.py`（e2b-local，挂载感知版） | **PASS 3 / FAIL 0 / SKIP 4**（1 跨用户、2 路径逃逸、5 出网 PASS；3/4/6 只读、7 uid SKIP） |

**新增的债**

1. **只读语义 / uid 隔离待 Cube 验收**：Cube 用 `volumeMounts[].readOnly: true`；e2b-local 忽略该字段（实测 rw），
   且以 root 跑、无 user namespace。本地已验证的是**挂载双向可见**（沙箱写 → 宿主可见、宿主写 → 沙箱可见）。
2. **worktree 在 light 下不生效（设计如此，不是缺陷）**：`worktree_create` 是 Tauri 命令（会话运行之外，没有环境绑定），
   删除走会话删除钩子，两者都跑在宿主上、行为正确；但 light 下工作区被固定成 `/workspace`，
   所以 worktree 配置被忽略。可选改进：light + managed worktree 时打一条 warn，或在 UI 上提示。
3. ~~`open_reader` 整文件下载~~ **已解决**：envd 的 `/files` 支持 `Range`（实测 206 + `Content-Range`），
   `RangeReader` 按 4 MiB 分块拉取；集成测试用 9 MiB 文件验证字节数一致。
4. **抽帧批量化（暂不做，2026-11-15 记录方案）**：现在一帧一次 `exec`（200 帧 ≈ 2–6 s 往返）。
   - **完整方案（4–6 h）**：新增 `StagedDir`（local 用 `tempfile::TempDir`；light 用沙箱 `/tmp/pointer-media-<id>/`，
     `Environment` 已有 `create_dir_all` / `list_dir` / `remove_dir_all`，无需加方法）→ 一次
     `ffmpeg -vf fps=<fps> frame-%03d.jpg` → **一次 `tar -cf - | base64` 取回**（200 帧 ≈ 8 MB，base64 ≈ 10.7 MB，
     需给该次调用把 `max_output_bytes` 从写死的 1 MiB 放宽到 16 MiB）。
   - **简化方案（10 分钟）**：把帧数上限从 200 压到 50，现状逐帧 exec 就只要 0.5–1.5 s。
   - **暂缓理由**：只影响延迟、不影响内存正确性（内存问题已由 `MediaInput` 路径化解决）。等 light 真机实测延迟再定。
5. **PDF 仍是进程内 pdfium（P2，暂不做）**：`media/pdf.rs` → `media/pdf_render.rs` 用 `pdfium_auto`，
   所以 PDF 必须整本进宿主内存（受 `MEDIA_INLINE_READ_MAX_BYTES` = 32 MiB 保护，超限**报错**而不是崩）。
   - **完整方案（1–2 天）**：`pdfinfo` 取页数 + `pdftoppm -jpeg -r <dpi> -scale-to 1400 -f -l` 在沙箱渲染，
     按 `env.kind()` 分派（local 保留 pdfium 快路径）；镜像需装 `poppler-utils` + **中文字体**。
   - **缩小方案（半天）**：只做 `pdftotext` 文本抽取 + 前 3 页页图，不做全量渲染对齐。
   - **卡点**：仓库没有任何 PDF fixture；镜像缺中文字体会让页图变豆腐块；pdfium 与 poppler 的裁切/字体回退不同，
     长期一致性要对齐。**需要先拿到一份真实中文 PDF 样本再动。**
6. **常驻进程（MCP / hooks）：已跑通，但 light 下插件目录还没挂进沙箱**。
   - **协议事实（2026-11-15 e2b-local 实测，踩过的坑记在这里）**：
     - `ProcessSelector` 的字段是 **`pid`**，不是 `processId`。写成 `processId` 会得到
       `501 unimplemented: invalid input type *process.ProcessSelector`（极具误导性，看起来像路由没实现）。
     - `Start` 的流事件是 `{"event":{"start":{"pid":N}}}` —— **pid 直接在 `start` 下**，没有 `process` 包一层；
       找错路径会表现为"30s 没收到 start 事件"，看起来像网关不流式。
     - `end` 事件在退出码为 0 时**不写** `exitCode`；被信号杀死时也没有，靠 `status`/`error` 里的 signal/killed 区分。
     - `SendInput` body `{"process":{"pid":N},"input":{"stdin":"<base64>"}}`；`CloseStdin` / `SendSignal` 同理。
   - 集成测试 `remote_spawn_streams_stdin_and_stdout` 在 e2b-local 上**真实通过**（起 `cat` → 写 → 关 stdin → 读回 → 退出码 0）。
   - **插件目录已挂进沙箱**：`{pointer_home}/plugins` → `/plugins`（ro）；hook 脚本路径经
     `PathMap::to_agent` 换成沙箱路径，映射不到就明确报错（不猜）。
     集成测试 `plugin_hook_script_runs_from_mounted_plugins_dir` 在 e2b-local 上真实通过：
     脚本只读挂载 → 路径映射 → 沙箱内执行 → 写事件 → 读回 block 决策 → 退出码 0。
7. `container` 档位（GUI）未开始。



1. **凭证如何进沙箱？** **已定：env 注入**（见下）。
   - **做法**：凭证作为请求级环境变量，由应用层在 spawn 时通过 `ExecOptions.env` 叠加（与 §3.3「请求注入变量」同一套机制），provider 不需要理解凭证语义。
   - **不选 tmpfs 挂载**：多一种挂载类型 = provider 多一处实现与权限面，且与"注入变量跨后端一致"冲突。
   - **不选出口代理注入**（本轮）：需要在沙箱外跑代理并改写请求，运维面大于收益；留作需要"沙箱永远看不到明文"时的可选增强。
   - **代价（必须遵守）**：注入值对沙箱内所有进程可见，agent 自己 `env` 就能打印。所以——
     只注入**该次请求真正需要**的凭证（请求粒度，不常驻会话）；日志与工具输出**不得**回显注入值；
     `/skills` 里的 `.env` 只承载技能脚本的**共享**环境配置（所有用户可见），不放个人凭证（§5.5）。
   - **验收**：§5.6 第 6 条（`find /skills -name 'token.txt' -o -name '*_token.txt'` 为空）。
2. **用户能不能改自己那份技能？** **已定：暂时只有 `local` 允许修改。**
   `light` / `container` 下 skills 一律只读，**不提供 per-user 可写副本**（也不做 overlayfs、不做"派生到用户库"）。
   - Web 端由 **admin 会话**承担维护：admin 默认解析为 `local`（§3.4），在宿主环境里直接改技能；
     改完**就地生效**（下次会话看到新版本；§5.5 不做版本目录）。
   - 代价：普通用户无法自助加技能/改脚本；桌面端用户回到 local 改完再发布。
   - 前提：技能运行时产物必须写 `$WORKING_DIR`（§5.5），否则只读挂载会让技能跑不起来。
   - 存储：因无 per-user 副本，skills 只占**一份**共享空间（本机 11 个技能按白名单约 30 MB），与用户数无关。
3. **是否支持跨用户共享项目目录？** 支持则需加 `shared/<project_id>` 层，权限模型复杂一档。
4. **宿主为 macOS/Windows 时怎么办？** Cube 需要 Linux + KVM，本机只能当客户端；沙箱集群必须部署在 Linux（裸金属或 PVM 云 VM）。
5. **Cube 能否暴露 noVNC 端口？** 阶段三的前置验证项，不通则 container 回退 Docker。
6. **Cube 部署形态：** 裸金属（性能最好）/ PVM 云 VM（无需嵌套虚拟化）/ K8s（v0.6+）——按现有机房条件选。

---

### 7.3 沙箱内交互式终端（console PTY，已落地 2026-11-18）

**落地情况**：三个后端都实现了 `pty_open`（宿主 `portable_pty`、envd 走
`StartRequest.pty`、docker 包一层 `docker exec -it`），`console_session` 按会话解析
环境后直接调用，**早期那个"沙箱档位不支持交互式终端"的门禁已经不存在**（文档之前
一直没更新）。agent 的 `terminal` 工具走 `env.exec`，与这条路径无关。

**容器档位实测**（`container_tier_it::console_pty_opens_inside_the_container`）：

| 断言 | 结果 |
|---|---|
| 是真 tty（不是管道） | `stty size` → `30 100` ✅ |
| 命令落在沙箱内 | `pwd` → `/workspace` ✅ |
| resize 传到容器内 tty | `40 120` ✅ |

**修掉的缺陷**：容器 PTY 的 resize 之前**只改宿主 pty**，`docker exec` 内层 tty 不变
——容器里 `stty size` 永远是打开时的值，vim/htop 会按错的行列渲染。原因是不依赖
SIGWINCH 传递。现在启动时用 `sh -c 'tty > /tmp/.pointer-pty-<token>; exec "$@"'`
记下内层 pts 路径，resize 时额外 `docker exec <c> stty -F <pts> rows/cols`。
envd 走 `Process.Update` 本来就没有这个问题。

**协议实测（2026-11-15，e2b-local，按官方 e2b SDK 2.10.2 的 `process.proto`）**：

`pty` 是 **`StartRequest` 上的字段**，不在 `ProcessConfig` 里——放错层会被静默忽略，
表现为"传了 pty 但 `tty` 仍报 not a tty"（第一次探测就踩了这个坑）。

```proto
message StartRequest { ProcessConfig process = 1; optional PTY pty = 2; optional string tag = 3; optional bool stdin = 4; }
message PTY { Size size = 1; }              // Size { cols, rows }
message UpdateRequest { ProcessSelector process = 1; optional PTY pty = 2; }   // ← resize
message ProcessInput  { oneof input { bytes stdin = 1; bytes pty = 2; } }
message ProcessEvent.DataEvent { oneof output { bytes stdout = 1; bytes stderr = 2; bytes pty = 3; } }
```

| 探测 | 结果 |
|---|---|
| `{"process":{…}, "pty":{"size":{"cols":100,"rows":40}}}` | ✅ `/dev/pts/0`，`stty size` → `40 100` |
| 同上但 `pty` 塞进 `process` 里 | ❌ 静默忽略（`not a tty`） |
| 输出通道 | `data.pty`（base64），不是 `stdout` |
| `SendInput {"input":{"pty":"<base64>"}}` | ✅ 输入进 pty，回显正常 |
| `Update {"process":{pid},"pty":{"size":{"cols":120,"rows":50}}}` | ✅ **resize 生效**（`stty size` → `50 120`） |
| 兜底（后端不支持 pty 时） | 沙箱内 `script -qfc '<cmd>' /dev/null` 也能造 tty（`/dev/pts/0`，`isatty True`） |

**参考实现（2026-11-15 读源码）**：

| | 沙箱终端怎么起 | tty 由谁给 | resize |
|---|---|---|---|
| OpenClaw | `docker exec -i [-t] -w <workdir> -e … <container> <cmd>`，stdin 始终 `pipe-open` | **容器运行时**（`docker exec -t`） | 无 |
| Hermes | `docker exec [-i] <container> bash -c <cmd>`，stdin 走管道或 heredoc | **没有 tty** | 无 |

两家的共同点：**客户端不做 pty**。OpenClaw 把 tty 交给容器运行时，Hermes 干脆不要；
`node-pty` 在 OpenClaw 里只用于**本地**进程，沙箱路径不经过它。

**Cube 上是否一样**：Cube 是 E2B 兼容 API，exec 路径是
`CubeAPI → Master → Cubelet → Shim → envd`，**末端就是同一个 envd**，所以 `pty` 字段
走的是同一份 `process.proto`。差异只可能出现在 envd 版本（Cube 侧镜像里的 envd 是否
带 pty 支持）——**必须在真机上跑一次 §7.3 验收命令确认**，不能只靠"接口兼容"推断。

**设计**：

1. `Environment` 增加 `pty_open(spec) -> Box<dyn PtyHandle>`（`LocalEnv` 用
   `portable_pty`；`RemoteEnv` 在 `StartRequest` 上带 `pty`，输出读 `data.pty`、
   输入写 `input.pty`，resize 走 `Process.Update`）。
2. 窗口尺寸：启动时用 `PTY.Size{cols,rows}`；前端 resize 事件 → `Process.Update`，
   实测生效（无需重启会话）。
3. `console_session` 在非 Local 档位改走 `env.pty_open`，不再拒绝。
4. 限制（必须写进 UI 提示）：依赖 `isatty` 的提权/密码交互（sudo/ssh）仍不建议在面板里用；
   全屏程序重绘需前端按 `data.pty` 做终端仿真（xterm.js 级别），不要自己拼 ANSI。

**验收**：真机在沙箱会话打开终端 → `tty` 返回 `/dev/pts/N`、`pwd` 为 `/workspace`、
`echo $((1+1))` 正常；宿主 `ps` 里看不到该 shell。

**估时**：接口 + LocalEnv 1 天；RemoteEnv `script` 包装 + 流对接 0.5 天；
前端 resize/提示 0.5 天。

### 7.4 档位 / 后端分离 + Docker 后端（P0–P2 已落地）

#### 7.4.1 要解决的问题

现状把两件事塞进了一个枚举：

```rust
pub enum EnvProvider { Local, Light }
```

它同时回答「该不该隔离」和「隔离怎么实现」。后果是**换实现必须换语义**：

- 生产要 Cube（envd 协议 + `host-mount`）
- 开发只有 e2b-local（envd 协议 + 卷目录）
- 而 e2b-local 的卷只能是 `~/.e2b-local/volumes/` 下的真实目录，**挂不了用户的项目目录**

于是开发环境里 `/workspace` 是每会话空卷：agent 在空目录干活、`git status` 无内容、
diff 无基线、文件树面板空。开发和生产的**语义一致但能力残缺**，验出来的东西对不上。

实测过的三条路都不通（2026-11-15）：

| 尝试 | 结果 |
|---|---|
| `volumes/x` → 项目目录的符号链接 | 网关拒绝：`volume path ... is not a directory` |
| `metadata["host-mount"]`（Cube 通道） | 网关静默忽略 |
| 网关配置加 bind | 配置只有 `traffic` / `docker.envd_binary` / `docker.volume_host_path` |

参考实现（Hermes / OpenClaw）都不走"卷"抽象，直接 bind 宿主目录：
Hermes `-v {host_cwd}:/workspace`（另有沙箱目录 / tmpfs 两级回落），
OpenClaw `-v {workspaceDir}:{workdir}`（`workspaceAccess: rw/ro/none`）。

#### 7.4.2 目标与不变式

```
生产：  Tier::Light + Backend::Envd    → Cube（项目目录走 host-mount）
开发：  Tier::Light + Backend::Docker  → 本地 docker -v 项目目录
```

不变式（改完必须仍成立）：

1. 工具层零改动（48 个工具、`effective_env()`、`ToolContext` 不动）
2. fail-closed 保留（未绑定 → 拒绝执行，不落宿主）
3. `remote_env_it` 的 8 个集成测试继续通过（Envd 后端协议不受影响）

#### 7.4.3 数据模型

```rust
pub enum Tier { Local, Light, Container }        // 语义：决定 Capabilities
pub enum Backend { Host, Envd, Docker }          // 实现：决定 IO 怎么落地

pub struct ProviderConfig {
    pub tier: Option<Tier>,        // 原 provider
    pub backend: Option<Backend>,  // 新增
    // sandbox_url / volume_host_path / template / admins / accounts / platform 不变
}

pub struct ResolvedEnv { pub tier: Tier, pub backend: Backend }
```

配置项：

| 环境变量 | 值 | 说明 |
|---|---|---|
| `POINTER_SERVER_ENVIRONMENT_PROVIDER` | `local` / `light` / `container` | 保留原名，映射到 `Tier` |
| `POINTER_SERVER_ENVIRONMENT_BACKEND` | `host` / `envd` / `docker` | **新增**，缺省按下表推导 |
| `POINTER_SERVER_ENVIRONMENT_SANDBOX_URL` | url | 仅 `envd` 后端需要 |
| `POINTER_SERVER_ENVIRONMENT_TEMPLATE` | image | `envd` / `docker` 共用 |
| `POINTER_SERVER_ENVIRONMENT_VOLUME_HOST_PATH` | dir | 仅 e2b-local 需要 |
| `POINTER_SERVER_ENVIRONMENT_DESKTOP` | `1` / `0` | **envd 专用**：模板自带图形桌面（Xvfb + xdotool + x11vnc）时打开。打开后该沙箱的 `capabilities.desktop` 为真，计算机工具走沙箱内的 xdotool 而不是宿主；不打开时桌面工具不下发 |

后端缺省推导（保证旧配置行为不变）：

| Tier | 未显式指定 backend 时 |
|---|---|
| Local | `Host` |
| Light | 有 `sandbox_url` → `Envd`；否则报错并提示两条出路 |
| Container | `Docker` |

`admins` / `accounts` 覆盖规则不变（仍是 tier 级覆盖）。

#### 7.4.4 DockerEnv 设计

核心洞察：`/workspace` 是项目目录的 **bind mount**，宿主和容器看到同一份文件。所以

- **文件 IO 不进容器** → 直接复用 `LocalEnv`（先把 guest 路径按 `PathMap` 翻回宿主路径）
- **只有执行进容器** → `docker exec`

| 方法 | 实现 |
|---|---|
| `exec` | `docker exec -i <name> <argv>`；`-w` 走 `ExecOptions.cwd`，env 用 `-e` |
| `spawn` | `docker exec -i` 常驻进程，stdin/stdout 走管道 |
| `pty_open` | 宿主 `portable_pty` 起 `docker exec -it <name> <shell>`（tty 由 `-t` 提供，与 OpenClaw 一致） |
| `read_file` / `write_file` / `list_dir` / `walk` / `metadata` / `remove_*` | 路径映射到宿主后交给 `LocalEnv` |
| `kind()` / `capabilities()` | `EnvKind::Light` / `Capabilities::light()` |

挂载列表**复用 `MountPlan` 的输出**：`Mount { host, guest, read_only }` → `-v host:guest[:ro]`。
Envd 与 Docker 两条后端共享同一份挂载计算，不各写一套。

容器生命周期：

```bash
docker run -d --name pointer-sbx-<conv> \
  -v <host_workspace>:/workspace \
  -v <host_home>:/home/pointer \
  -v <skills_user>:/skills/user:ro \
  -v <plugins>:/plugins:ro \
  --workdir /workspace <template> sleep infinity

docker rm -f pointer-sbx-<conv>   # destroy，幂等
```

#### 7.4.5 会话环境分派

```rust
let ResolvedEnv { tier, backend } = cfg.resolve(Some(user))?;
match (tier, backend) {
    (Tier::Local, _)      => Ok(None),
    (_, Backend::Envd)    => acquire_remote(cfg, conv, user, workspace_root),
    (_, Backend::Docker)  => acquire_docker(cfg, conv, user, workspace_root),
    (_, Backend::Host)    => bail!("tier 非 Local 却指定 Host 后端"),
}
```

缓存仍按 `conversation_id`，条目记录 `backend`；空闲淘汰时按 backend 调对应 `destroy()`。

#### 7.4.6 项目目录挂载（前置改动）

`MountPlan` 必须接收项目目录，否则两条后端都挂不上用户的项目：

```rust
pub fn for_session_with_plugins(
    sandboxes_base: &Path,
    session_user_id: &str,
    conversation_id: &str,
    skill_roots: &[SkillRoot],
    plugin_root: Option<&Path>,
    workspace_root: Option<&Path>,   // 新增：会话所属项目目录
) -> Vec<Mount>
```

`/workspace` 来源二选一：有项目目录且**不是 session-sandbox 目录** → 挂项目目录；
否则退回每会话空卷（"从零开始"语义）。

> 已部分落地（`mounts.rs` / `session_env.rs` / `commands.rs::panel_env` 已加参数，待全量测试）；
> 由于 e2b-local 的限制，该改动在开发环境仍表现为空卷，需要 Docker 后端才能真正生效。

#### 7.4.7 测试计划

| 层 | 测试 | 覆盖 |
|---|---|---|
| 单测 | `tier_backend_resolution_matrix` | 9 种组合 + 旧配置兼容（`provider=light` + url → Envd） |
| 单测 | `workspace_mount_uses_project_dir` / `falls_back_to_volume` | 项目目录进挂载 / 无项目退回卷 |
| 单测 | `docker_env_maps_paths_both_ways` | 宿主↔guest 双向映射，含越界拒绝 |
| 集成（`--ignored`，需 docker） | `exec_runs_in_container` | `pwd` = `/workspace`，`uname` 是 Linux |
| 集成 | `host_sees_container_writes` | 容器内写文件 → 宿主立即可见（bind 验证） |
| 集成 | `pty_gets_tty` | `tty` = `/dev/pts/N`，`stty size` 生效 |
| 集成 | `destroy_removes_container` | `docker ps` 中不再出现 |
| 回归 | `cargo test -p pointer-core --lib` | 基线不降（当前 2295） |
| 回归 | `remote_env_it` | 8 个仍过 |

#### 7.4.8 风险

| 风险 | 应对 |
|---|---|
| uid 不一致（容器 root 写文件 → 宿主属主异常） | 实测；必要时 `--user $(id -u):$(id -g)` |
| 容器泄漏（崩溃残留） | 启动时清理 `pointer-sbx-*` 孤儿；`destroy` 幂等 |
| docker 不可用 | 创建时明确报错 + 提示 `docker info` |
| 路径映射错位（最严重，会写错地方） | 单测锁死双向映射；架构守卫测试覆盖 `docker.rs` |
| 与 e2b-local 并存 | 按 backend 二选一，互不影响 |

#### 7.4.9 分阶段交付

| 阶段 | 内容 | 估时 |
|---|---|---|
| P0 | `Tier`/`Backend` 模型 + 配置解析 + 旧配置兼容 + 单测 | 0.5 天 |
| P1 | `env/docker.rs`：exec / spawn / fs / pty + 单测 | 1 天 |
| P2 | `session_env` 分派 + 缓存 / 销毁 + 集成测试 | 0.5 天 |
| P3 | 开发环境默认切 docker 后端 + 真机验面板 / diff / 终端 | 0.5 天 |
| P4 | 文档更新 + 架构守卫白名单补 `docker.rs` | 0.5 天 |

P0 完成后旧配置行为完全不变，可随时停。

#### 7.4.10 实施记录（2026-11-15）

| 阶段 | 状态 | 证据 |
|---|---|---|
| P0 档位/后端模型 | ✅ | `provider.rs`：`Tier` / `Backend` / `ResolvedEnv`；`resolve()` 返回二元组；后端缺省按档位推导；档位与后端不自洽时报错 |
| P0 配置 | ✅ | 新增 `POINTER_SERVER_ENVIRONMENT_BACKEND`（`host`/`envd`/`docker`）；旧配置 `provider=light` + `sandbox_url` 行为不变 |
| P1 DockerEnv | ✅ | `env/docker.rs`：exec / spawn / pty_open / 文件 IO（复用 LocalEnv）/ 容器生命周期 |
| P2 分派与缓存 | ✅ | `session_env::acquire_docker` + 会话级缓存 + 单飞锁；`env_for_conversation` 同时查两份缓存 |
| P3 开发环境切换 | ✅ | `pointer.local.env` 加 `BACKEND=docker`；真机 `uname -s` → `Linux`，`pwd` → `/workspace` |
| P4 文档 | ✅ | 本节 |

**真机验证（开发版 DEV 窗口）**：

```
$ uname -s
Linux
$ pwd
/workspace
```

容器挂载实测：

```
/Users/starliu/.agents/skills/agent-browser -> /workspace (rw=true)   ← 项目目录直接 bind
/Users/starliu/.pointer/plugins            -> /plugins  (ro)
/Users/starliu/.e2b-local/volumes/_anonymous-home -> /home/pointer
```

**踩过的坑（已修）**：`env_for_conversation` 最初只查 envd 缓存，docker 后端的会话在工具线程重放时查不到环境，退到 `DeniedEnv`，
表现为 `执行环境未绑定：…（exec 已拒绝，未在宿主执行）`。修复后统一查两份缓存。

**收尾（同日完成）**：

| 项 | 做法 |
|---|---|
| 孤儿容器清理 | `DockerEnv::cleanup_orphans()` 只清 `status=exited` 的（在跑的不动），`lib.rs::run` 起线程调用一次 |
| 缓存空闲淘汰 | `docker_cache` 条目带 `last_used`；`evict_idle_docker` 到 30 分钟先 `docker rm -f` 再移出 |
| 面板不抢建沙箱 | docker 后端下 `panel_env` 直接读宿主（bind 挂载同一份文件），避免用旧路径定死挂载 |
| 终端面板误报 | `console_session::create` 只查缓存；应用重启后缓存空 → 落到 `DeniedEnv` → 默认 `pty_open` 报「(Light) 不支持交互式终端」。改为查不到时按会话解析/创建；`DeniedEnv` 也覆盖了 `pty_open`，报「执行环境未绑定」 |
| 架构守卫 | 守卫扫描范围是 `tools/`、`media/`、`agents/coder/`，`env/` 不在其中，无需加白名单（`cargo test --lib` 已覆盖） |

#### 7.4.11 明确不做

| 项 | 原因 |
|---|---|
| ~~Container 档位的 GUI（Xvfb + WM）~~ | ✅ 已做（§6.7），2026-11-18 又补了窗口铺满 |
| 快照能力 | 仍未做（`Capabilities::container().snapshot = false`） |
| ~~资源限制（cpu / mem / disk）~~ | ✅ 已做：`--cpus` / `--memory` / `--storage-opt size=`（§6.4 第 5、10 条） |
| Cube 真机验证 | 另一条线，需要 Cube 环境（§5.6） |
| 修 e2b-local 的挂载限制 | 不碰第三方网关，dev 改用 docker 后端绕开 |

### 7.5 面板内接管电脑（VNC，已落地 2026-11-18）

**需求（用户口径）**：右侧面板加一个「接管电脑」选项。**点了它，人和鼠标键盘才接管**；
不点就只是看。不做自动仲裁，不做租约，不做双屏。

**已定的边界**：

| 项 | 决定 | 理由 |
|---|---|---|
| 观看 vs 接管 | 两种需求分开 | 「看」用截图轮询就够；「接管」才需要实时流 |
| 仲裁（agent 与人抢指针） | **不做** | 一个「接管电脑」开关本身就是入口约束 |
| 双虚拟屏 / XInput2 多指针 | **不做** | 场景不成立，投入不值 |
| iframe 嵌 `vnc.html` | **不做** | 工具栏割裂、缩放不可控 |
| 自建 Rust WS 桥 | **不做** | 容器内 `websockify` 一行解决 |

**方案**：

```
容器：Xvfb :99 → x11vnc :5900 → websockify :6080（--web /usr/share/novnc）
宿主：-p 127.0.0.1::6080
前端：@novnc/novnc（npm 包，`new RFB(canvas, wsUrl)`），不用 iframe
```

`Environment::desktop_endpoint()` 原来只是 trait 默认实现（恒 `None`），`DockerEnv` 里的
`vnc_credentials()` 是死代码——所以「看沙箱桌面」按钮从来没出现过。现在改成返回结构体
`DesktopEndpoint { vnc_url, ws_url, password }`，两个读者（`get_sandbox_desktop` /
`open_sandbox_desktop`）同步更新。

**落地结果**：

| 层 | 改动 | 验证 |
|---|---|---|
| 镜像 | apt 装 `websockify` + `novnc`（Debian 1.3.0，静态文件在 `/usr/share/novnc`） | `curl /vnc.html` → 200，`/core/rfb.js` → 200 |
| 入口 | `entrypoint-container.sh` 起 `websockify --web=/usr/share/novnc 6080 127.0.0.1:5900` | `pgrep -af websockify` 有进程 |
| 端口 | `run_args` 多映射 `-p 127.0.0.1::6080`；`SPEC_VERSION` 3→4（否则旧容器按指纹复用，新端口不生效） | 单测断言两个 `-p` |
| Rust | `published_port()` 统一解析 `docker port`；新增 `ws_endpoint()`；`DockerEnv` 实现 `desktop_endpoint()` | `container_tier_it::desktop_endpoint_exposes_websockify` |
| 命令 | 新增 `desktop_env()`：只认**已存在**的容器，不抢建沙箱（`panel_env` 在 docker 后端故意返回宿主环境，不能复用） | `desktop_tests` 2 个用例 |
| 前端 | `SandboxDesktopPanel.vue`（RFB + 适应/1:1 + 重连 + 断开即释放）；`WorkspacePanel` 新增 `desktop` 视图与 tab；`@novnc/novnc@1.5.0` | 组件测试 4 个用例 |

**真实链路实测（2026-11-18）**：用容器自带的 noVNC 客户端连 `ws://127.0.0.1:<port>/websockify`：

- 状态栏「已连接到 <容器名>:99」，canvas 1280×577
- canvas 像素与容器桌面左上角区域**100% 一致**（逐像素比对，无差异）
- 鼠标点击后容器 `xdotool getmouselocation` 跟随（1:1 直通）
- 逐字符按键在 xterm 里执行 `whoami` → `pointer`

**已知风险与现状**：

1. **Tauri CSP**：`tauri.conf.json` 的 `csp` 是 `null`，不挡 `ws://127.0.0.1:*`，原担心不成立。
   将来加 CSP 需要放行 `connect-src ws://127.0.0.1:*`。
2. **分辨率**：桌面固定 1920×1080。面板默认「适应」；「1:1」时 `clipViewport` + `dragViewport`
   可拖动平移。
3. **中文输入**：noVNC 发 keysym，宿主输入法不生效——这条**仍未做**，需要走容器 X 剪贴板
   （`xclip`）+ Ctrl+V。
4. **web 模式**：浏览器端连不到宿主 `127.0.0.1`，`web.ts` 仍返回 `null`，本次不做。
5. **空闲回收**：VNC 是浏览器↔容器直连，不刷新 `last_used`，接管期间若别处触发淘汰，
   30 分钟后容器仍会被回收，WS 断开——面板会显示「连接已断开，点重连继续」。**未做 pin。**
6. **`open_sandbox_desktop`**：保留为退路（面板工具栏「用系统屏幕共享打开」）。

### 7.6 容器内浏览器 profile 与启动参数（已落地 2026-11-17/18）

**改造前（2026-11-16 实测）**：

| 事实 | 证据 |
|---|---|
| 容器以 root 运行，`HOME=/root` | `docker exec … echo $HOME` |
| Chromium profile 落在 `/root/.config/chromium` | `docker diff` 显示可写层新增 301 条 |
| `/home/pointer` 挂载的是 `session-sandboxes/<用户段>-home` | `mounts.rs:100`；envd 下是 `<volume_host_path>/<用户段>-home` |
| 该目录当前为空（0 B），**Chromium 完全没用到它** | `ls _anonymous-home` 无内容 |
| `browser` 是 `/usr/local/bin/browser` 包装脚本 | `docker/browser.sh` |

**当时暴露的两个问题**（现已全部修掉，见文末「已落地」）：

1. **登录态丢失**：Cookie / localStorage 在容器可写层，容器一旦重建（30 分钟空闲回收、
   规格指纹变化、手动清理）全部丢失。应用重启但容器还在时能保留。
2. **包装可被绕过**：`/usr/bin/chromium` 是 Debian 的包装脚本（5066 字节），真正二进制在
   `/usr/lib/chromium/chromium`。直接调 `chromium` 会硬失败：

   ```
   ERROR: Running as root without --no-sandbox is not supported.
   ```

   `xdg-open` 同样绕过包装。漏掉 `--disable-dev-shm-usage` 时容器 `/dev/shm` 只有 64MB，
   标签页会随机 SIGBUS 崩溃——表现为“页面突然白屏”，很难归因。

**方案：把包装下沉到 `/usr/bin/chromium`**

```
/usr/bin/chromium          → 我们的包装（全部必需参数 + --user-data-dir）
/usr/lib/chromium/chromium → 真二进制，不动
```

参数清单（`docker/browser.sh` 里那些，全部下沉）：

```sh
# 窗口尺寸按当前屏幕算；真正的铺满靠 openbox 规则（见下）
SIZE=$(xdpyinfo 2>/dev/null | awk '/dimensions:/{print $2; exit}')
[ -n "$SIZE" ] || SIZE=1600x1000

exec /usr/lib/chromium/chromium \
    --disable-dev-shm-usage \
    --disable-gpu \
    --user-data-dir=/home/pointer/.config/chromium \
    --no-first-run --no-default-browser-check \
    --disable-features=Translate,MediaRouter \
    --window-size="$SIZE" --window-position=0,0 \
    "$@"
```

注意：`browser.sh` 内部必须改成调 `/usr/lib/chromium/chromium`，否则自己调自己会无限递归。

**验证过的点**：把 `--user-data-dir` 指向 bind 挂载目录后，profile 确实落到宿主
（实测 `/tmp/prof-home/.config/chromium` 有内容），容器重建后仍在。

**待定的细节**：`/home/pointer` 是**按用户**的，不是按会话的。

| 方案 | 效果 | 代价 |
|---|---|---|
| `/home/pointer/.config/chromium` | 同用户所有会话共享登录态 | 两会话同时开浏览器会 profile 锁冲突 |
| `/home/pointer/.chromium/<会话ID>` | 按会话隔离 | 登录态不跨会话复用 |

倾向按用户共享（登录一次到处能用），锁冲突时让后来的会话复用已有实例。

**工作量**：约半天（镜像改动 + 重建 + 集成测试断言 profile 落在挂载目录）。

**另记（同批排查，未定）**：`user_settings.json` 是单文件、无用户维度
（`storage.rs:653` 的 `load_user_settings()` 不带用户参数），多用户部署下模型选择、
providers、调试开关全部共享；调试会话设置更是只在内存（`app_state.rs:1334`）。
桌面版单用户不构成问题，server 多用户需要按用户拆分。待产品定位确定后再排。

**补充调研（2026-11-16）：容器内用 root 还是普通用户**

业界立场：

| 来源 | 结论 |
|---|---|
| Docker / 各家安全指南 | 非 root 是标准做法，root 是头号反模式 |
| Bitnami | 专发 `non-root` 镜像变体；OpenShift 直接不接受 root 镜像 |
| linuxserver.io | 更细致：容器**以 root 启动**，初始化后降权到普通用户跑应用；并指出“任何时刻都不能是 root”多半是对隔离边界的误解 |
| Playwright 官方 | 默认 root 跑浏览器 → **禁用 Chromium 沙箱**；可信代码可以，**访问不可信站点必须建独立用户 + seccomp profile** |

**但我们的特殊性**：agent 本来就通过 `docker exec` 以 root 在容器里跑任意命令。
浏览器降权不能改变信任模型——真正的边界是**容器 ↔ 宿主**（读写挂载、无 egress 限制、共享内核）。
浏览器沙箱的价值是纵深防御：恶意网页利用漏洞时被限制在沙箱内，而不是直接拿到容器 root。

**非 root 的四个实际障碍（本机实测）**：

| 障碍 | 实测结果 | 处理 |
|---|---|---|
| `apt` | `E: Could not open lock file /var/lib/dpkg/lock-frontend`（权限拒绝） | 只能构建期装好 |
| `/tmp/.X11-unix` | `_XSERVTransmkdir: ERROR: euid != 0, directory /tmp/.X11-unix will not be created` → **桌面起不来** | Dockerfile 预建并 `chmod 1777` |
| 挂载目录属主 | 宿主 uid（501/1000）与容器 uid 不一致 | 需 PUID/PGID 重映射或 entrypoint 里 chown |
| `pip` / `npm` | ✅ 均可用（`pip install --user`、`npm install` 落 `$HOME`） | 无需 root |

`apt` 的四种应对：

| 做法 | 评价 |
|---|---|
| 构建期装全 | **主力方案**，运行时不需要 apt |
| entrypoint 以 root 起、装完降权 | 只适合固定初始化，应对不了 agent 临时装包 |
| 应用侧 `docker exec -u 0` 显式提权 | 可行，需审计（当前 `docker.rs:430` 的 exec 没有 `-u`，要加参数） |
| 装 sudo / NOPASSWD | 不推荐，等于把 root 还回去 |

**三种配置对比**：

| 配置 | 网页漏洞 → 攻击者拿到 | 成本 |
|---|---|---|
| A 现在：root + `--no-sandbox` | 容器 root | 0 |
| B 降权跑应用（linuxserver 模式） | 容器内普通用户 | 中：entrypoint 处理 uid 映射 + X11 目录 |
| C 全程非 root + seccomp | Chromium 沙箱内 | 高：挂载权限、seccomp profile、X11 权限 |

**决定（2026-11-16）：采用 B，对齐 Cursor 云 agent 的模型**——非 root 用户（uid 1000）
+ sudo，不再走“保持 root”这条路。

理由：Cursor 的云 agent 默认用户是 `ubuntu`（非 root）且带 sudo，Docker 官方的
cursor-agent 沙盒模板也强制 uid 1000 / `USER agent`。他们这么做的前提是隔离边界足够强
（独立 VM），而我们的共享内核容器里，非 root 的边际收益本来更小——但实测下来成本也不高，
而且能换回 Chromium 沙箱，值得做。

**已落地的改动**：

| 文件 | 改动 |
|---|---|
| `docker/sandbox-container.Dockerfile` | 装 `sudo`；`pointer ALL=(ALL) NOPASSWD:ALL`；预建 `/tmp/.X11-unix` 1777；末尾 `USER pointer` |
| `docker/sandbox-light.Dockerfile` | 同上（无桌面，但同样非 root） |
| `docker/browser.sh` | 去掉 `--no-sandbox`；改成直接调 `/usr/lib/chromium/chromium`，参数写在脚本里 |
| `docker/entrypoint-container.sh` | 启动时清理浏览器 profile 的 `SingletonLock`（见下） |

**为什么是替换 `/usr/bin/chromium`，而不是设 `CHROMIUM_FLAGS` 环境变量**：

一开始按文档设想用了 `ENV CHROMIUM_FLAGS=...`，实测**参数根本没到浏览器**——Debian 的
`/usr/bin/chromium` 第 13 行写死了 `CHROMIUM_FLAGS=""`，会把镜像 ENV 重置掉。

改为用 `docker/browser.sh` 覆盖 `/usr/bin/chromium`（真二进制在 `/usr/lib/chromium/chromium`），
再软链成 `/usr/local/bin/browser`。这样直接敲 `chromium`、`xdg-open`、桌面文件
**全都走同一套参数**，绕不过去——解决了前面说的“包装可被绕过”问题。

```sh
exec /usr/lib/chromium/chromium \
    --disable-dev-shm-usage --disable-gpu \
    --user-data-dir=/home/pointer/.config/chromium \
    --no-first-run --no-default-browser-check \
    --disable-features=Translate,MediaRouter \
    --window-size="$SIZE" --window-position=0,0 \
    "$@"
```

`--user-data-dir` 指向挂载的用户 home，登录态在容器重建后仍在。

**窗口铺满屏幕**：`SIZE` 取自 `xdpyinfo`，但真正保证铺满的是 openbox 规则
（`docker/openbox-rc.xml` → `/etc/xdg/openbox/rc.xml`）：

```xml
<application class="Chromium-browser">
  <decor>no</decor>
  <maximized>yes</maximized>
</application>
```

为什么要 WM 强制：Chromium 会把上次的窗口尺寸记进 profile，持久化 profile 之后
`--window-size` 会被记忆值覆盖——实测同一个 profile 下传 `1920,1080` 拿到的还是
`1600x1000`。`class` 必须写 `Chromium-browser`：WM_CLASS 第一个字段（instance）
带 profile 路径，第二个（class）才稳定。

**home 挂载点为什么从 `/home/u` 改成 `/home/pointer`**

原来 guest home 是个叫 `/home/u` 的占位目录——容器里没有 `u` 这个用户，`HOME` 是
`/root`（root 时代）或 `/home/pointer`（现在），所以**没有任何进程会往 `/home/u` 写**，
那个挂载一直是空的。

改成 `/home/pointer` 之后，挂载点就是容器用户真正的 home：

| 好处 | 说明 |
|---|---|
| 路径自洽 | `HOME=/home/pointer`，`~/.config`、`~/.local`、`~/.cache` 全落在挂载上 |
| `pip install --user` / `npm install` 持久化 | 实测落到宿主（`~/.local/lib/python3.12`） |
| 浏览器 profile 不需要额外参数 | 默认就是 `$HOME/.config/chromium` |

**本节结论：已落地（2026-11-18）**

| 原来的问题 | 现在的状态 |
|---|---|
| 容器 root、profile 在 `/root/.config/chromium`，重建即丢 | 非 root（`pointer`，uid 1000）+ `HOME=/home/pointer` 挂载，登录态跨容器保留 |
| 包装可被绕过（直接敲 `chromium` 丢参数） | `docker/browser.sh` 覆盖 `/usr/bin/chromium`，`xdg-open` / 桌面文件同一套参数 |
| 窗口尺寸写死 1600×1000 | openbox 规则铺满屏幕（1920×1080），`SIZE` 从 `xdpyinfo` 取 |

验证：`container_tier_it` 7 个用例（含 `browser_starts_after_container_recreation`）全过。

**仍待定（本节遗留）**：

| 项 | 内容 |
|---|---|
| home 粒度 | `/home/pointer` 按用户不按会话。同一用户两会话同时开浏览器会 profile 锁冲突；倾向"按用户共享 + 复用已有实例"，entrypoint 已清理跨容器 `SingletonLock` |
| `user_settings.json` | 单文件、无用户维度（`storage.rs:653`），多用户部署下模型选择 / providers / 调试开关全共享。待产品定位确定后再排 |

代价是**挂载会盖掉 `useradd -m` 生成的 skel 文件**（`.bashrc`/`.profile`）。
entrypoint 里补了一段：空目录时从 `/etc/skel` 拷回来，只在缺失时写，不覆盖用户文件。

**连带发现：profile 锁会跨容器残留**

profile 落到挂载目录后，Chromium 留下的 `SingletonLock`（内容是指向旧容器 hostname
的软链）会跟着存活。新容器里 Chromium 直接拒绝启动：

```
The profile appears to be in use by another Chromium process (44)
on another computer (4038781f7db5). Chromium has locked the profile ...
```

实测：重建容器后窗口数 0。entrypoint 里加了清理——**只在锁属于别的 hostname 时**删
（同机多实例时保持原锁不动），清理后窗口数恢复 2。回归测试
`browser_starts_after_container_recreation` 覆盖这条。

**实测（非 root 下，全部通过）**：

| 项 | 结果 |
|---|---|
| 默认用户 | ✅ `uid=1000(pointer)`，`HOME=/home/pointer` |
| `sudo` | ✅ `whoami` → root |
| 写宿主挂载目录 | ✅ `WRITE_OK` / `APPEND_OK` |
| Xvfb + openbox | ✅ `1920x1080` |
| x11vnc | ✅ 正常监听 |
| **Chromium 带沙箱渲染** | ✅ 沙箱报错 0 条，页面渲染正常 |
| profile 落盘 | ✅ `/home/pointer/.config/chromium`（4.1 MB） |
| 容器重建后浏览器 | ✅ 锁被清理，窗口正常出现 |
| `apt-get install` | ❌ 权限拒绝（需 `sudo`） |
| `pip install --user` / `npm install` | ✅ |

**测试**：

```
cargo test -p pointer-core --test container_tier_it -- --ignored --test-threads=1  → 7 passed
cargo test -p pointer-core --test docker_env_it -- --ignored --test-threads=1      → 10 passed
cargo test -p pointer-core --lib                                                  → 2312 passed, 10 ignored
```

**已知代价**：

- `sudo` 让 agent 仍能提权——这是 Cursor 的取舍（可审计、日常最小权限），
  但它不是安全边界；真正的边界仍是容器↔宿主。
- Linux 宿主上 bind mount 属主与 uid 1000 不一致时需要 PUID/PGID 映射（macOS 实测无此问题）。
- **技能目录里的裸凭证文件会让环境解析直接失败**：`verify_skill_roots` 命中即报错，
  `~/.pointer/skills/cwpt-reimburse-submit/token.txt` 就会挡住整个 container 档位。
  跑集成测试前需要把它挪走。这是当前最容易被踩到的一个坑。

## 9. 开发环境与验证记录

### 9.0 从零准备开发环境（约 10 分钟）

**适用**：macOS + Apple Silicon（M1 及以上）。Linux 开发机跳过 OrbStack，直接用系统 Docker 即可。

**为什么不用 Docker Desktop**：`--network host` 在 Docker Desktop 里是 VM 的 netns，不转发到 macOS。e2b-local 需要网关与沙箱容器共享 `127.0.0.1`，所以必须 OrbStack。

#### 步骤 1：安装 OrbStack

brew 直装会走 CDN，实测限速到 1.3 KB/s（45 分钟只下 3.6 MB）。走代理：

```bash
# 1. 查本机代理端口（Clash/Surge 等）
scutil --proxy | grep -E 'HTTPPort|HTTPSPort'      # 例：33210

# 2. 查当前版本的下载地址
brew info --json=v2 --cask orbstack | python3 -c \
  "import sys,json;print(json.load(sys.stdin)['casks'][0]['url'])"

# 3. 代理直下（实测 1.4 MB/s，448 MB 约 85 秒）
export https_proxy=http://127.0.0.1:33210
curl -L -o /tmp/OrbStack.dmg "<上一步输出的 URL>"

# 4. 安装（卷名带空格，注意引号）
hdiutil attach /tmp/OrbStack.dmg -nobrowse -quiet
cp -R "/Volumes/Install OrbStack v2.2.5/OrbStack.app" /Applications/
hdiutil detach "/Volumes/Install OrbStack v2.2.5" -quiet
xattr -dr com.apple.quarantine /Applications/OrbStack.app

# 5. 启动（首次会装特权 helper）
open /Applications/OrbStack.app
```

#### 步骤 2：切 Docker 上下文

```bash
docker context use orbstack
docker info --format '{{.OperatingSystem}}'    # 应输出 OrbStack
```

#### 步骤 3：起本地沙箱

```bash
cd <repo>
scripts/dev-sandbox.sh up
```

首次会拉镜像并提取 envd 二进制（约 1 分钟），之后秒级。

#### 步骤 4：验证

```bash
scripts/dev-sandbox.sh status   # runtime=OrbStack，health=OK
scripts/dev-sandbox.sh smoke    # exec 与文件读写都通过
```

期望输出：

```
  sandbox=sbxxxx  envd=http://127.0.0.1:32779
  exec -> ok
aarch64
  file -> hello
  冒烟通过
```

#### 常见问题

| 现象 | 原因 | 处理 |
|---|---|---|
| `docker info` 显示 Docker Desktop | 上下文没切 | `docker context use orbstack` |
| 网关起来了但 `curl` 连不上 | 用了 `-p` 而非 `--network host` | 必须 host 网络 |
| exec 返回 `exit 127`，stderr 含 `nice: not found` | 镜像缺 coreutils | 用 debian/ubuntu，**不要用 alpine** |
| 沙箱创建失败 | 镜像没拉 | `docker pull debian:bookworm-slim` |
| 磁盘告警 | 镜像与沙箱层累积 | `docker system prune -af` |

### 9.1 本地沙箱（`scripts/dev-sandbox.sh`）

开发机（macOS + Apple Silicon）无需 KVM 即可验证 Environment 抽象层。

```bash
scripts/dev-sandbox.sh up       # 启动网关（提取 envd、生成 config、等健康）
scripts/dev-sandbox.sh status   # 检查 runtime 与 health
scripts/dev-sandbox.sh smoke    # 建沙箱 + exec + 读写文件
scripts/dev-sandbox.sh bench    # 跑基准（docs/zh-CN/design/bench_e2b.py）
scripts/dev-sandbox.sh clean    # 清理残留的沙箱容器
scripts/dev-sandbox.sh down     # 停止网关
```

产物落在 `.dev-sandbox/`（envd 二进制 + config.yaml），**已在 `.gitignore` 中忽略**。

**硬前提**：Docker 必须是 **OrbStack**。脚本会检查 `docker info` 的 OperatingSystem，不是 OrbStack 直接报错并给出切换命令。

**一个踩过的坑**：如果端口上已有别的实例在跑，健康检查会被"骗过"——新容器绑定失败退出，但 `curl /health` 仍然 200。脚本现在先校验容器 `State.Running` 再探健康，端口冲突时直接报错并列出占用进程。

### 9.2 能验 / 不能验

| 阶段 | 本地可验 | 说明 |
|---|---|---|
| P1 Environment 抽象 + LocalEnv | ✅ 完全 | 不需要沙箱 |
| P2 路径映射（正/反向） | ✅ | 纯逻辑，单测 |
| P2 文件 IO 走 API | ✅ | 对 e2b-local 跑 |
| P2 批量走 exec | ✅ | 已实测 142–146 倍差距 |
| P2 exec / PTY / streaming | ✅ | envd 是 E2B 官方二进制 |
| P3 工具层重构 | ✅ | 协议一致即可 |
| P4 GUI + noVNC | ⚠️ 部分 | `envdURL` 可用，`get_host` 语义需真 Cube |

| 必须真 Cube | 原因 |
|---|---|
| 控制面多跳延迟 | e2b-local 是单进程 Go 网关，无 CubeAPI→Master→Cubelet→Shim |
| **Volume / Host Mount 语义** | e2b-local 用 Docker named volume；Cube 用 `hostPath` + virtiofs |
| `cache: never` 行为 | virtiofs 特有 |
| 冷启动 60 ms | 本地实测 1354 ms（容器启动） |
| 快照 / 回滚（CubeCoW） | 无 |
| 多租户 uid + mount namespace 隔离 | 单用户 VM，验不了 |

**最大缺口是 Volume 语义**——映射表（`/workspace`→Volume、`/skills`→Host Mount）在本地跑的是 Docker volume，验不出 Cube 的 `hostPath` 行为。

### 9.3 开发机限制

| 限制 | 影响 |
|---|---|
| 磁盘 13 GB 可用 | 多镜像测试会很快见底，需定期 `docker system prune` |
| 仅 arm64 | 跑不了 `linux/amd64` 镜像（QEMU 模拟慢 5–10 倍） |
| 嵌套虚拟化（Mac→OrbStack VM→容器） | 测出的延迟是**上界**，真实 Cube 更低 |

### 9.4 验证时间线

| 步骤 | 结论 |
|---|---|
| 确认 Cube 不能本机跑 | macOS 无 `/dev/kvm`，M1 Pro 不支持嵌套虚拟化，磁盘 13 GB |
| 找到 e2b-local | CubeSandbox 官方 OMA 文档推荐，云端 Cube / 本地 e2b-local 同一套接口 |
| 拉镜像验证内容 | `ghcr.io/superduck-ai/e2b-local` 内含 Go 网关 + `envd-linux-{amd64,arm64}` |
| Docker Desktop 失败 | `--network host` 是 VM netns，不转发到 macOS |
| 安装 OrbStack | brew 走 CDN 限速 1.3 KB/s；改用代理直下 DMG（1.4 MB/s）成功 |
| 网关跑通 | `health=200`，`POST /sandboxes` 返回 `envdURL` |
| 首次基准无效 | alpine 缺 `/usr/bin/nice`，所有 exec 返回 127；换 debian 后有效 |
| 最终数据 | exec 3.3 ms、单文件 2–4 ms、批量差 142–146 倍 |

### 9.5 复现要点

- `config.yaml` 只需两段：`traffic.advertised_host` 与 `docker.envd_binary`（绝对路径）
- `envd_binary` 必须是**宿主可见路径**，容器内路径 daemon 看不到
- 网关监听地址默认 `127.0.0.1:3000`，`addr` 配置键未生效（实测）
- exec 走 Connect server-streaming：`Content-Type: application/connect+json`，body 为 `[0x00][len:4BE][json]`
- 文件读写走 REST：`POST/GET /files?path=...&username=root`

---

## 10. 参考资料

- `crates/pointer-core/src/agents/mod.rs` — 提示词模板渲染
- `crates/pointer-core/src/tools/file/path.rs` — 路径解析
- `crates/pointer-core/src/session_sandbox.rs` — 用户/会话目录模型
- 开发环境：`scripts/dev-sandbox.sh`（一键起停本地 E2B 沙箱，见 §9）
- 性能实测脚本：`bench_io.py`（本地 fs vs HTTP vs `docker exec`）、`bench_e2b.py`（真实 envd），§2.3 数据由后者产出
- CubeSandbox 存储架构：virtiofs 共享宿主目录、overlay rootfs、Volume Plugin（`hostPath` 边界）
- noVNC / Selkies：GUI 旁路方案选型
- Codex CLI sandbox 设计（`read-only` / `workspace-write` / `danger-full-access` 三档 + 审批正交）
- openclaw 沙箱后端（配置期校验、pinned 路径操作）
- WorkBuddy 实测：会话级容器沙箱，共享宿主内核，`nproc` 泄漏宿主规格
