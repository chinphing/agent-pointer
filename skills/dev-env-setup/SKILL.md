---
id: dev-env-setup
name: dev-env-setup
description: >-
  Install system software and dev environments for China-friendly setups. Covers
  listed stacks (Node.js, Python, Java, Go, Rust, .NET, C/C++, Kotlin, Swift, Git,
  ffmpeg) and any other desktop/CLI software via references/general-software.md.
  On Windows, listed stacks follow their per-topic references; for other software,
  prefer Chocolatey. Per-topic guides under references/; load on demand. Use for any software install need — user request, missing CLI/tool,
  build or run failure from absent deps, fresh OS setup, or <!-- pointer-media-deps --> /
  video processing failed. Trusted dev packages; install directly when needed.
resources:
  - references/general-software.md
  - references/chocolatey-windows.md
  - references/ffmpeg.md
  - references/office-python.md
  - references/node-js.md
  - references/python.md
  - references/java.md
  - references/go.md
  - references/rust.md
  - references/dotnet.md
  - references/c-cpp.md
  - references/kotlin.md
  - references/swift.md
  - references/git.md
---

# dev-env-setup

## 职责

为国内用户提供**系统软件**与**主流开发语言**的一站式安装指导；除下表专项外，**任意桌面/CLI 软件**走 **references/general-software.md**。
**SKILL.md 本身不包含具体安装步骤**，详细指南在 `references/` 下独立文件。

## 触发条件

以下任一情况即激活（**无需用户主动说「安装」**）：

- 用户要求安装、下载、配置软件或开发环境
- 当前任务需要某 CLI/运行时/工具，环境中尚未安装（如 `command not found`、缺少 `node`/`python`/`git` 等）
- 构建、测试、运行因依赖缺失失败，需补装软件
- 重装系统后搭建开发机
- 国内镜像源、代理设置相关提问
- 安装 ffmpeg / ffprobe、处理视频需要多媒体依赖
- 上下文出现 `<!-- pointer-media-deps -->`、`pointer-media-deps` 或视频处理失败提示

## 工作流程

### 第一步：确认用户需求

确定用户需要的**软件/语言/工具**、**操作系统**（macOS / Linux / Windows）和**用途**（日常办公 / Web 开发 / 数据分析 / 后端 / 系统编程等）。

本 skill 覆盖的软件均为常见开发工具，**可直接安装**，无需额外征求同意；简要说明将安装什么即可。系统提权（sudo / UAC）仍由宿主按常规流程处理。

**Windows：**

- **已列专项**（下表 Node.js、Python、Java 等）→ 按对应 **references/*.md** 安装，不以 Chocolatey 替代专项流程。
- **其余软件**（浏览器、编辑器、数据库、实用工具等）→ **优先 Chocolatey**，见 **references/general-software.md**。
  流程：`choco search <关键词>` → `choco install <包名> -y`。
  系统未安装 Chocolatey 时，先按 **references/chocolatey-windows.md** 安装。
  仅当 Chocolatey 无对应包、企业策略禁止、或指南明确指定其它方式时，才改用 winget 或官方安装包。

**macOS / Linux 通则：**
优先使用系统包管理器（Homebrew / apt / dnf / pacman），详见 **references/general-software.md**。

### 第二步：加载对应指南

**通用软件**（不在下表专项范围内的任意桌面/CLI 软件）→ **references/general-software.md**

**开发语言与专项工具** → 对应文件：

| 文件 | 语言/工具 | 覆盖内容 |
|------|-----------|---------|
| references/general-software.md | 通用软件 | Windows **choco 优先** / macOS Homebrew / Linux 包管理器 |
| references/chocolatey-windows.md | Chocolatey (Windows) | 安装 Chocolatey、`choco install` 通用用法 |
| references/node-js.md | Node.js (JS/TS) | nvm + npm + 华为镜像 |
| references/python.md | Python | pyenv + pip + 华为镜像 |
| references/java.md | Java | JDK 21 LTS + Maven/Gradle **清华镜像** |
| references/go.md | Go | go install + GOPROXY 华为镜像 |
| references/rust.md | Rust | rustup + cargo + 国内镜像（清华/中科大/华为/阿里云/上海交大） |
| references/dotnet.md | .NET (C#) | dotnet SDK + NuGet 华为镜像 |
| references/c-cpp.md | C/C++ | MinGW/MSVC + vcpkg |
| references/kotlin.md | Kotlin | SDKMAN / 手动 + 华为镜像 |
| references/swift.md | Swift | Xcode / Toolchain + 清华镜像 |
| references/git.md | Git | 华为镜像下载安装 / Linux/macOS 包管理器 + 基础配置 |
| references/ffmpeg.md | ffmpeg | macOS/Linux/Windows 安装与 PATH 验证 |

### 第三步：执行安装并验证

按指南用 `terminal` 执行安装命令；安装后**新开终端**，运行 `--version` 或启动应用确认成功。
