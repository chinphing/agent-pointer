---
name: dev-env-setup
description: 国内环境下安装主流开发语言（Node.js/Python/Java/Go/Rust/.NET/C/C++）。当用户说"装环境"、"配置开发环境"、"安装XX"时触发。每个语言有独立安装指南，SKILL.md 仅做索引。
metadata:
  tags:
    - environment
    - development
    - china
    - setup
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

确定用户需要的**语言**、**操作系统**（macOS / Linux / Windows）和**用途**（Web 开发 / 数据分析 / 后端 / 系统编程等）。

### 第二步：加载对应语言指南

根据用户需求，从 `references/` 加载对应文件：

| 文件 | 语言 | 覆盖内容 |
|------|------|---------|
| references/node-js.md | Node.js (JS/TS) | nvm + npm + 华为镜像 |
| references/python.md | Python | pyenv + pip + 华为镜像 |
| references/java.md | Java | JDK 21 LTS + Maven/Gradle 华为镜像 |
| references/go.md | Go | go install + GOPROXY 华为镜像 |
| references/rust.md | Rust | rustup + cargo + 华为镜像 |
| references/dotnet.md | .NET (C#) | dotnet SDK + NuGet 华为镜像 |
| references/c-cpp.md | C/C++ | macOS Xcode CLT / Linux GCC / MinGW 华为镜像 |
| references/kotlin.md | Kotlin | SDKMAN / 华为镜像手动下载 |
| references/swift.md | Swift | macOS Xcode / 清华镜像 |

调用方式：`skill_read_resource` 读对应文件内容。

### 第三步：按指南逐步执行

将指南内容呈现给用户，配合终端工具执行安装命令。指南中的命令已适配华为云镜像，可直接运行。

### 第四步：验证安装

安装后建议验证：
- `node --version && npm --version`
- `python --version && pip --version`
- `java --version`
- 依此类推

## 约束

- **所有包管理器 / 安装命令均已切换到华为云镜像**（`mirrors.huaweicloud.com`）。
- Windows 不再使用 winget/scoop 等包管理器安装，改为直接从华为镜像下载压缩包手动配置。
- 不涉及 IDE 安装（VS Code / IntelliJ 等），聚焦运行时和包管理器。
- 如用户明确需要使用国外源，尊重用户选择。
- Swift 因华为镜像暂未覆盖，使用清华镜像作为替代。
