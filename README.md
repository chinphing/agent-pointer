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

## 开发

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

加载规则采用三层渐进式披露：第一层只把 `name` 和 `description` 作为 Skill 索引注入上下文；当模型判断任务需要某个 Skill 时，通过 `load_skill_instructions` 加载第二层 `SKILL.md` 正文；当正文引用 `references/`、`assets/` 或 `scripts/` 下的文件时，可通过 `read_skill_resource` 按需读取第三层资源。frontmatter 只支持官方字段：`name`、`description`、`license`、`compatibility`、`metadata`、`allowed-tools`。

导入后会自动刷新 Skills 列表；启用外部 Skill 后，其说明会参与当前对话。zip 中的脚本或二进制不会被自动执行。



## 打包与发布


本项目已配置 Tauri 2 跨平台打包，相关配置位于：

- `src-tauri/tauri.conf.json`：应用名称、窗口、bundle、Windows/macOS/Linux 打包配置
- `.github/workflows/release.yml`：GitHub Actions 三端自动构建发布流程
- `package.json`：本地开发、图标生成、打包脚本

### 本地打包

首次打包前安装依赖：

```bash
npm install
```

生成各平台所需图标资源：

```bash
npm run icons
```

当前系统打包：

```bash
npm run tauri:build
```

也可以使用语义化脚本，需在对应系统执行：

```bash
npm run build:windows
npm run build:macos
npm run build:linux
```

安装包输出目录：

```bash
src-tauri/target/release/bundle/
```

### 各平台构建要求

- Windows：需要 Node.js、Rust、Microsoft C++ Build Tools、WebView2 Runtime
- macOS：需要 Node.js、Rust、Xcode Command Line Tools；macOS 安装包建议在 macOS 上构建
- Linux：需要 Node.js、Rust、WebKitGTK 等系统依赖；CI 使用 `ubuntu-22.04`

Ubuntu/Debian 可参考：

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

### GitHub Actions 三端自动打包

已配置 `.github/workflows/release.yml`，会分别在以下 runner 上构建：

- `windows-latest`：Windows 安装包
- `macos-latest`：macOS 通用包，使用 `--target universal-apple-darwin`
- `ubuntu-22.04`：Linux 安装包

触发方式：

1. 手动触发：GitHub 仓库 `Actions` 页面运行 `Release`
2. 发版触发：推送版本标签，例如：

```bash
git tag v0.1.0
git push origin v0.1.0
```

CI 会自动执行：

1. 拉取代码
2. 安装 Node.js 20
3. 安装 Rust stable
4. Linux 安装 WebKitGTK 等依赖
5. 执行 `npm install`
6. 执行 `npm run icons`
7. 使用 `tauri-apps/tauri-action` 构建安装包
8. 上传产物到 GitHub Draft Release

### 图标说明

源码图标为：

```bash
src-tauri/icons/icon.svg
```

打包前会通过以下命令生成 Tauri 所需图标：

```bash
npm run icons
```

会生成包括 `32x32.png`、`128x128.png`、`128x128@2x.png`、`icon.ico`、`icon.icns` 在内的平台图标文件。

### 签名与公证说明

当前配置可生成未签名安装包。正式发布时建议补充：

- Windows：代码签名证书
- macOS：Apple Developer 证书、应用签名、公证 Notarization
- Linux：根据发布渠道决定是否补充包签名
