# Pointer · AI 工作台

基于 **Tauri 2 + Vue 3 + TypeScript** 的桌面大模型聊天客户端，默认接入 **千问（DashScope OpenAI 兼容模式）**，支持工具调用（function calling）与 Skills 技能扩展。

## 特性

- 多轮聊天 / 流式回复 / 停止生成 / 重试
- 默认 OpenAI 兼容协议；默认 Provider：千问 `qwen-plus`
  - Base URL：`https://dashscope.aliyuncs.com/compatible-mode/v1`
- 工具调用：Rust 侧 Tool Registry，默认自动允许调用，可在设置中改为敏感工具二次确认
- Skills：按三层渐进式方式提供技能索引、正文说明与资源读取；支持通过 `zip` 导入外部 Skills

- 兼容 Web：Vue 界面可运行在浏览器中，Web 后端通过 `server` crate 复用 `crates/pointer-core`
- API Key 通过 Tauri 后端保存（避免暴露到前端运行时）

## 文档

设计与使用说明等见 **[`docs/README.md`](docs/README.md)**（含 `design/`、`internals/`、`guides/` 等分类目录）。

## 开发

界面约定（助手消息 `thoughts` / API `reasoning` / 竖线进度 /「原始输出」面板）见 [`docs/ui/assistant-message-ui.md`](docs/ui/assistant-message-ui.md)，修改对应 Vue 逻辑前请先阅读，避免回归。

桌面端：

```bash
npm install
npm run tauri:dev
```

Web 端：

```bash
npm run server:dev
npm run dev:web
```

首次启动后，在「设置」中填入 DashScope API Key（在阿里云百炼控制台获取），即可对话。

官网登录（Release 安装包）已内置 `pointer.readflowai.com` / `pointer-api.readflowai.com` / `pointer-som.readflowai.com`，无需配置环境变量；本地开发（`tauri dev`）默认连本机 3000/8001/8000，仍可用 `POINTER_*` 覆盖。

## 运行架构

```text
Vue 统一界面
├─ 桌面端：Tauri Adapter -> src-tauri -> crates/pointer-core
└─ Web 端：Web Adapter -> server(axum HTTP/SSE) -> crates/pointer-core
```

`crates/pointer-core` 复用模型协议、Provider、工具注册表、Skills、聊天编排和本地存储逻辑；`src-tauri` 仅保留桌面壳与 Tauri IPC 适配，`server` 提供 Web 端 HTTP/SSE API。

## 外部 Skills

外部 Skills 参考 Claude Agent Skills 的三层渐进式规范：每个 Skill 是一个目录，目录中包含带 YAML frontmatter 的 `SKILL.md`，可选包含 `references/`、`scripts/`、`assets/` 等资源目录。

加载来源按优先级从高到低：

1. 当前工作目录 `skills/`
2. 当前工作目录 `.agents/skills/`
3. 用户目录 `~/.agents/skills/`
4. 应用数据目录 `PointerApp/skills/`

同名 Skill 冲突时，高优先级来源覆盖低优先级来源。Windows 下应用数据目录通常位于：

```text
%APPDATA%/PointerApp/skills
```

支持通过「Skills 技能库」里的「导入 zip」按钮导入到 `PointerApp/skills/`。zip 中每个 Skill 必须是一个 kebab-case 目录，并包含精确命名的 `SKILL.md`；不支持 `skill.md`、`skill.json` 或 `manifest.json`。

推荐的 `SKILL.md` 格式：

```markdown
---
name: translator
description: Translates, polishes, and standardizes terminology between Chinese and English. Use when users ask for translation, rewriting, localization, or terminology consistency.
metadata:
  tags:
    - translation
---

# Translator Skill

当用户要求翻译、润色或统一术语时使用该技能。
保持原意，优先使用自然、准确、符合目标语言习惯的表达。
```

加载规则采用三层渐进式披露：第一层只把 `name` 和 `description` 作为 Skill 索引注入上下文；当模型判断任务需要某个 Skill 时，通过 **`skill_read`**（仅 `skill_id`）加载第二层 `SKILL.md` 正文；当正文引用 `references/`、`assets/` 或 `scripts/` 下的文件时，可通过 **`skill_read`**（带 `path`）按需读取第三层资源。frontmatter 只支持官方字段：`name`、`description`、`license`、`compatibility`、`metadata`、`allowed-tools`。

导入后会自动刷新 Skills 列表；启用外部 Skill 后，其说明会参与当前对话。zip 中的脚本或二进制不会被自动执行。



## 打包与发布

Windows / macOS / Linux 的环境准备、开发命令、本地打包与 GitHub Actions 发版，见 **[`docs/guides/cross-platform-build.md`](docs/guides/cross-platform-build.md)**。

简要命令（在 `pointer-app/` 目录；**须在对应操作系统上打包**，无法在一台机器上产出三端安装包）：

| 平台 | 命令 | 产物（`src-tauri/target/release/bundle/` 下） |
|------|------|-----------------------------------------------|
| Windows | `npm run build:windows` | `msi/*.msi` |
| macOS | `npm run build:macos` | `macos/*.app`、`dmg/*.dmg` |
| Linux | `npm run build:linux` | `deb/*.deb`、`appimage/*.AppImage` |

```bash
npm install
npm run icons          # 首次或更换图标后
npm run tauri:build    # 当前系统默认格式；Linux 经 scripts/tauri-build.mjs 自动 NO_STRIP
```

三端脚本均调用 `tauri:build`（仅 Linux 设置 `NO_STRIP=true`，避免 AppImage 的 linuxdeploy 失败）。  
CI 三端并行见 `.github/workflows/release.yml`。  
配置文件：`src-tauri/tauri.conf.json`。
