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
3. **容器内路径更干净**。agent 看到 `/workspace`、`/skills`、`/home/u`，不暴露宿主目录结构，可移植性更好。
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
| `{app_data}/session-sandboxes/{user}/` | `/home/u` | rw |
| `{app_data}/session-sandboxes/{conv}/` | `/workspace` | rw |
| `/data/skills-ro/` | `/skills` | ro |
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
    pub network: NetworkPolicy,
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

### 5.3 目录映射（路径映射）

| 宿主路径 | 容器内 | 权限 | Cube 挂载方式 |
|---|---|---|---|
| `{app_data}/session-sandboxes/{user}/` | `/home/u` | `rw` | Volume |
| `{app_data}/session-sandboxes/{conv}/` | `/workspace` | `rw` | Volume |
| `/data/skills-ro/` | `/skills` | `ro` | Host Mount（`readOnly: true`） |
| `/tmp` | `/tmp` | `rw` | tmpfs |

agent 只看到 `/workspace`、`/home/u`、`/skills`，不暴露宿主目录结构。

### 5.4 启动方式（Cube）

**关键约束：用户数据必须走 Volume / Host Mount，不能放 rootfs。**

rootfs 是 overlay（lower 只读 + upper 可写），宿主看到的是分离的两层，合并视图只存在于 guest 内；而 Volume / Host Mount 的底层就是宿主目录，宿主可以直接 `std::fs` 读写——这正是路径映射只需要"字符串翻译"的原因。

```python
from cubesandbox import Sandbox, Volume
import json

vol = Volume.create(f"ws-{tenant_id}")          # 每用户一个 Volume

mounts = json.dumps([
    {"hostPath": "/data/skills-ro", "mountPath": "/skills", "readOnly": True},
])

with Sandbox.create(
    template=TEMPLATE_ID,
    volume_mounts={"/workspace": vol, "/home/u": vol},
    metadata={"host-mount": mounts},
    env={"HOME": "/home/u", "PIP_USER": "1",
         "PYTHONUSERBASE": "/home/u/.local"},
) as sb:
    ...
```

**必须配置的两个点**：

| 项 | 值 | 原因 |
|---|---|---|
| virtiofs `cache` | **`never`** | `always` 会让宿主写入对 guest 不立即可见 |
| `host-mount` 前缀白名单 | 仅 `/data/skills-ro` 与 `session-sandboxes` | 默认只允许 `/data/shared/` 下路径，需收紧 |

### 5.4.1 skills 只读的双份保护

`readOnly: true` 只把 guest 侧的 bind mount remount 成 `MS_RDONLY`，**底层共享目录仍可被宿主写**。所以：

| 位置 | 机制 |
|---|---|
| guest 内 | `readOnly: true` + `MS_NOSUID/NOEXEC/NODEV` |
| 宿主进程 | `path.rs` 对 `skills_root` 显式拒绝写 |

### 5.5 skills 发布流水线

`~/.pointer/skills/<name>/` **不能整体挂载**，因为混了三类内容：

| 内容 | 处理 |
|---|---|
| `SKILL.md` / `references/` / `scripts/` / `assets/` | 发布到 `/data/skills-ro/` |
| `cases/` / `.cache/` / `.venv/` / `*.log` | **不发布**（每用户运行时产生） |
| `token.txt` / `.env` / `cwpt_token.txt` | **不发布**（凭证） |
| `.git/` / `.pytest_cache/` / `.ruff_cache/` | **不发布** |

发布命令（白名单）：

```bash
rsync -a \
  --include='SKILL.md' --include='references/***' \
  --include='scripts/***' --include='assets/***' \
  --exclude='*' \
  ~/.pointer/skills/<name>/ /data/skills-ro/<name>@<ver>/
```

**遗留问题**：技能运行时写 `cases/`（如 `cases/<单号>/facts/*.json`）。只读挂载会让技能跑不起来。两条路：

- **5.5.a（推荐）**：`cases/` 改写到 workspace 下的会话目录，技能文档同步修改
- **5.5.b**：skills 用 overlayfs（lower=只读代码，upper=每用户可写），保持 `{baseDir}` 语义

### 5.6 验收（可执行）

```bash
# 1. 跨用户不可见
sandbox exec <id> -- ls /home/u/../../{other_tenant}   # 期望：No such file or directory

# 2. 路径逃逸无效
sandbox exec <id> -- cat /workspace/../../etc/passwd   # 期望：不存在

# 3. skills 只读
sandbox exec <id> -- touch /skills/x                   # 期望：Read-only file system

# 4. 无法提权
sandbox exec <id> -- mount -o remount,rw /skills       # 期望：Operation not permitted

# 5. 无外网
sandbox exec <id> -- curl https://example.com          # 期望：失败

# 6. 凭证不可见
sandbox exec <id> -- find /skills -name 'token.txt' -o -name '.env'   # 期望：空
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
| 技能写 `cases/` 冲突 | 见 5.5 的 a/b 方案 |
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

**base 层清单**：`locales` + `zh_CN.UTF-8`、`fonts-noto-cjk`、`tzdata`（`Asia/Shanghai`）、python3-pip/venv、nodejs/npm、`gcc g++ make`、`imagemagick`、`poppler-utils`、`git curl wget jq ripgrep unzip tar`、`dbus`。

**锁版本，不用 latest**，否则不可复现。

**预装放镜像层，用户装的放 Volume**：

```bash
-e PIP_USER=1
-e PYTHONUSERBASE=/home/u/.local
-e NPM_CONFIG_PREFIX=/home/u/.npm-global
-e PATH=/home/u/.local/bin:/home/u/.npm-global/bin:$PATH
```

| 行为 | 落点 | 是否持久 |
|---|---|---|
| 镜像预装 | 镜像层（只读） | 跨会话、跨用户共享 |
| `pip install --user` / `npm i -g` | Volume | ✅ 持久 |
| `apt install` | sandbox 可写层 | ❌ 销毁即弃 |

**注意**：`apt install` 不持久，提示词里要引导 agent 优先用 `pip`/`npm`。

**每用户不需要独立镜像**：1 万用户 × 1.2 GB = 12 TB，且平台升级要重建 1 万份。共享镜像 + 每用户 Volume 即可。

---

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

---

## 8. 未决问题

1. **`cases/` 写哪里？** 见 5.5 的 a（改技能）vs b（overlayfs）。
2. **凭证如何进沙箱？** env 注入 / tmpfs 挂载 / 出口代理注入，三选一。
3. **用户能不能改自己那份技能？** overlayfs 允许（只影响自己），纯只读不允许。
4. **是否支持跨用户共享项目目录？** 支持则需加 `shared/<project_id>` 层，权限模型复杂一档。
5. **宿主为 macOS/Windows 时怎么办？** Cube 需要 Linux + KVM，本机只能当客户端；沙箱集群必须部署在 Linux（裸金属或 PVM 云 VM）。
6. **Cube 能否暴露 noVNC 端口？** 阶段三的前置验证项，不通则 container 回退 Docker。
7. **Cube 部署形态：** 裸金属（性能最好）/ PVM 云 VM（无需嵌套虚拟化）/ K8s（v0.6+）——按现有机房条件选。

---

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
