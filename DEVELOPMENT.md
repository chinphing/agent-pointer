# 开发调试说明

[English](docs/en/DEVELOPMENT.md) | 简体中文

本文档用于说明 `Pointer` 的本地开发、调试、检查与常见问题处理流程。

## 环境要求

### 通用依赖

- Node.js 20+
- npm
- Rust stable
- Tauri 2 CLI，由项目依赖提供

检查命令：

```bash
node -v
npm -v
rustc --version
cargo --version
```

### Windows 额外要求

- Microsoft C++ Build Tools
- WebView2 Runtime

### macOS 额外要求

```bash
xcode-select --install
```

### Linux 额外要求

Ubuntu/Debian：

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  pkg-config \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libpipewire-0.3-dev \
  libspa-0.2-dev \
  libclang-dev \
  libgbm-dev \
  libegl1-mesa-dev \
  libdrm-dev \
  libwayland-dev
```

## 安装依赖

在项目根目录执行：

```bash
npm install
```

未设置 `POINTER_EDITION` 时即未绑定控制面（standalone，纯本地）。要联调控制面时在 `pointer.local.env` 里设 `POINTER_EDITION=managed` 和三个域名（见 [docs/zh-CN/deploy/editions.md](docs/zh-CN/deploy/editions.md)）；写成别的值（含旧值 `official`）会让构建直接失败。

## 启动开发模式

### 桌面端

完整桌面应用调试：

```bash
npm run tauri:dev
```

或：

```bash
npm run tauri dev
```

这会同时启动：

- Vite 前端开发服务
- Tauri 桌面窗口
- Rust 后端服务

**与正式版并行运行：** Debug 构建（`tauri dev` / `cargo run`）默认使用独立数据目录 **`PointerAppDev`**，与正式安装的 **`PointerApp`** 完全隔离（对话、设置、登录态、任务板等互不影响）。三平台路径示例：

| 平台 | 正式版 | 开发版 |
|------|--------|--------|
| **macOS** | `~/Library/Application Support/PointerApp/` | `…/PointerAppDev/` |
| **Linux** | `$XDG_DATA_HOME/PointerApp/`（默认 `~/.local/share/PointerApp/`） | `…/PointerAppDev/` |
| **Windows** | `%APPDATA%\PointerApp\` | `%APPDATA%\PointerAppDev\` |

首次 `tauri dev` 会新建空目录，需在开发实例里重新配置模型与登录。

可选环境变量（优先级从高到低）：

| 变量 | 作用 |
|------|------|
| `POINTER_APP_DATA_DIR` | 绝对路径，完全指定数据目录 |
| `POINTER_APP_DATA_SUBDIR` | 覆盖 `data_dir()` 下的子目录名（如 `PointerAppDev`） |

Release 构建（`tauri:build` 产物）始终使用 **`PointerApp`**，除非设置了上述环境变量。

**附件与本地媒体路径：** 统一经 `pointer_core::media::resolve_local_media_path` 解析。持久化附件的 `storageRelPath`（形如 `{conv}/{id}_{fileName}`）只会在 `{app_data}/conversation-media/` 下查找，**不会**相对进程 `cwd`（如 `src-tauri/`）解析，避免 dev 与数据目录错位。

### Web 端

Web 端复用同一套 Vue 界面，后端复用 `crates/pointer-core`，由 `server` crate 提供 HTTP/SSE API。

启动 Rust Web 后端：

```bash
npm run server:dev
```

另开一个终端启动 Web 前端：

```bash
npm run web:dev
```

默认同源：`WEB_API_BASE` 为空，Vite 把 `/api` 代理到 `127.0.0.1:8787`（需先起 `server:dev`）。pointer-server CORS 默认关闭；`tauri:dev` 不启该代理（桌面走 IPC）。

若前端直连 API（跨源），需同时打开 CORS：

```bash
POINTER_SERVER_CORS_ORIGINS=* npm run server:dev
VITE_WEB_API_BASE=http://127.0.0.1:8787 npm run web:dev
```

## 单独调试前端

```bash
npm run dev
```

然后打开终端输出的本地地址。

注意：单独在浏览器中运行时，Tauri 的 `invoke`、事件监听、文件存储等桌面能力不可用，完整功能请使用 `npm run tauri:dev`。

## 打开开发者工具

Tauri 窗口中可使用快捷键打开前端 DevTools：

- Windows/Linux：`Ctrl + Shift + I`
- macOS：`Cmd + Option + I`

可用于查看：

- Console 日志
- Network 请求
- Vue 运行错误
- Tauri `invoke` 调用异常
- 流式事件处理状态

## 模型配置调试

首次启动后，在应用「设置」中填写 DashScope API Key。

默认配置：

```text
Provider: qwen
Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
Model: qwen-plus
```

建议调试流程：

1. 打开设置
2. 填入 DashScope API Key
3. 点击测试连接
4. 发送简单消息，例如 `你好`
5. 测试工具调用，例如 `帮我计算 123 * 456`

## 外部 Skills 调试

外部 Skills 严格采用官方 Claude Skills 的目录式规范。加载来源按优先级从高到低：

1. `~/.pointer/skills/`（用户库；安装 **`skill_import`**，修改委派 **coder** 子 agent）
2. `~/.agents/skills/`（Codex / Agent 兼容，只读）
3. `{data_dir}/PointerApp/skills/`（bundled 系统库）

工作区 `skills/` 不参与运行时加载。详见 `docs/user/skills.md`（用户）与 `docs/developer/skills-compatibility.md`（格式规范）。

Windows 应用数据目录通常为：

```text
C:\Users\<用户名>\AppData\Roaming\PointerApp\skills
```

调试方式：

1. 准备包含 kebab-case Skill 目录和精确命名 `SKILL.md` 的 zip 包。
2. 启动桌面端或 Web 端。
3. 打开「Skills 技能库」。
4. 点击「导入 zip」。
5. 导入后列表中会显示「外部」标识。

`SKILL.md` 示例：

```markdown
---
name: translator
description: Translates and standardizes terminology between Chinese and English. Use when users ask for translation, polishing, or terminology consistency.
metadata:
  tags:
    - translation
---

# Translator Skill

当用户要求翻译或润色时使用。保持原意，输出自然准确的目标语言表达。
```

说明：`name` 和 `description` 是第一层 frontmatter 索引，且 `name` 必须与 Skill 目录名一致；不支持 `skill.md`、`skill.json`、`manifest.json` 或旧字段 `id`、`systemPrompt`、`toolNames`。启用 Skill 后不会立即注入完整正文，模型会在需要时通过 **`skill_read`**（`skill_id` + 必填 `path`，读说明时为 `SKILL.md`）读取第二层 `SKILL.md` 正文；`references/`、`assets/`、`scripts/` 下的文件作为第三层资源，通过 **`skill_read`**（同一 `skill_id` + 资源相对 `path`）按需读取。不会执行 zip 中的任意代码。




## Rust 后端调试


进入 Tauri 后端目录：

```bash
cd src-tauri
```

检查 Rust 编译：

```bash
cargo check
```

运行单元级编译检查时，建议先回到项目根目录执行完整开发命令：

```bash
npm run tauri:dev
```

开启详细日志：

### PowerShell

```powershell
$env:RUST_LOG="debug"; npm run tauri:dev
```

### Bash/zsh

```bash
RUST_LOG=debug npm run tauri:dev
```

## 前端检查

类型检查与前端构建：

```bash
npm run build
```

如果只需要启动 Vite：

```bash
npm run dev
```

## 图标与打包

跨平台环境、开发、打包与 CI 发版见 **[`docs/contributing/cross-platform-build.md`](docs/contributing/cross-platform-build.md)**。

打包前快速检查：

```bash
npm run icons
npm run tauri:build          # 当前系统
# 或：build:windows / build:macos / build:linux（须在对应 OS 上执行）
```

产物：`src-tauri/target/release/bundle/`（Windows `msi/`、macOS `dmg/`+`macos/`、Linux `deb/`+`appimage/`）。

## 常见问题

### 1. `tauri` 命令找不到

先安装依赖：

```bash
npm install
```

然后使用项目脚本：

```bash
npm run tauri:dev
```

不要依赖全局 `tauri` 命令。

### 2. `npx` 或 `npm` 找不到

说明 Node.js 未安装或未加入 PATH。安装 Node.js 20+ 后重新打开终端。

### 3. Windows 编译失败，提示 C++ 工具链缺失

安装 Microsoft C++ Build Tools，并勾选 C++ 桌面开发相关组件。

### 4. Linux 编译失败，提示 WebKitGTK 缺失

安装 Linux 系统依赖，见 [`docs/contributing/cross-platform-build.md`](docs/contributing/cross-platform-build.md) 的 Linux 章节。

### 5. API 请求失败

检查：

- API Key 是否已填写
- Base URL 是否为 `https://dashscope.aliyuncs.com/compatible-mode/v1`
- 模型名是否可用，例如 `qwen-plus`
- 网络是否能访问 DashScope 服务

### 6. 工具调用无响应

检查前端 DevTools Console 和终端日志，重点关注：

- `chat://stream` 事件
- `tool_call_start`
- `tool_call_result`
- `approve_tool_call`

## 推荐开发流程

```bash
npm install
npm run tauri:dev
```

修改代码后：

```bash
npm run build
cd src-tauri
cargo check
```

准备打包前，见 [`docs/contributing/cross-platform-build.md`](docs/contributing/cross-platform-build.md)：

```bash
npm run icons
```

### 7. `vue-tsc`：`number` is not assignable to `Timeout`

项目同时引入 DOM 与 `@types/node` 时，`ReturnType<typeof setTimeout>` 会变成 Node 的 `Timeout`，而 `window.setTimeout` / `window.setInterval` 返回的是 `number`。

前端浏览器定时器句柄请存为 `number`（或 `ReturnType<typeof window.setTimeout>`），与现有 `saveTimer` / `composerDraftTimer` 写法一致。

## 文档目录约定

| 路径 | 读者 | 内容 |
|------|------|------|
| [`docs/user/`](docs/user/README.md) | Pointer 终端用户 | 安装、Skills、IM、云主机、子 Agent 设置 |
| [`docs/developer/`](docs/developer/README.md) | 外部开发者、Skill 作者 | 通道部署、Skill 格式、扩展钩子、协议 |
| [`docs/contributing/`](docs/contributing/README.md) | 仓库贡献者 | 跨平台构建、平台 UI、离线评测 |
| [`docs/internals/`](docs/internals/README.md) | 核心维护者 | 运行时内部机制、任务板等 |
| [`docs/design/`](docs/design/README.md) | 评审 / 规划 | 设计方案、路线图、实现计划 |

新增文档时先确定读者：用户教程 → `user/`；第三方集成 → `developer/`；本仓库开发 → `contributing/` 或 `internals/` / `design/`。

```bash
npm run tauri:build
```
