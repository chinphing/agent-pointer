---
id: dev-env-setup
name: dev-env-setup
description: 国内环境下安装主流开发语言（Node.js/Python/Java/Go/Rust/.NET/C/C++/Kotlin/Swift/Git）。每个语言有独立安装指南，references/ 下按需读取。
resources:
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

为国内用户提供主流开发语言的一站式环境安装指导。**SKILL.md 本身不包含具体安装步骤**，各语言的详细指南在 `references/` 下独立文件。

## 触发条件

用户表达以下意图时激活：
- "安装 xx 环境" / "配置开发环境"
- "我想学 xx，先装什么"
- 重装系统后要搭开发机
- 国内镜像源、代理设置相关提问

## 工作流程

### 第一步：确认用户需求

确定用户需要的**语言/工具**、**操作系统**（macOS / Linux / Windows）和**用途**（Web 开发 / 数据分析 / 后端 / 系统编程等）。

### 第二步：加载对应语言指南

根据用户需求，从 `references/` 加载对应文件：

| 文件 | 语言/工具 | 覆盖内容 |
|------|-----------|---------|
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
