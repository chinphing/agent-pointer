# 架构说明

[English](../../en/developer/architecture.md) | 简体中文

桌面端和 Web 端共用一套对话与工具实现。

```text
Vue 界面
├─ 桌面：Tauri Adapter → src-tauri → crates/pointer-core
└─ Web：Web Adapter → server (HTTP/SSE) → crates/pointer-core
```

## crate 地图

工作区成员在根 `Cargo.toml` 的 `[workspace].members` 里声明，共 5 个：

| crate / 目录 | 类型 | 产物 | 职责 |
| --- | --- | --- | --- |
| `crates/pointer-core` | **lib（无 bin）** | — | 全部业务核心：对话引擎、工具执行、LLM provider、Skills / 插件 / MCP、Agent、调度、存储、License |
| `crates/pointer-channels` | **lib（无 bin）** | — | IM 通道适配器（飞书 / 钉钉 / 企微 / 微信）+ 配对 + 出站投递 |
| `server/` | bin | `pointer-server` | axum HTTP/SSE + 同源 Vue `dist`，Web 端与自建部署 |
| `src-tauri/` | bin | `pointer-app`（桌面产物名 `Pointer`） | Tauri 2 桌面壳：IPC、托盘、更新器、云主机 WebView、macOS 权限 |
| `tools/license-gen/` | bin | `license-gen` | License 签发工具 |

> ⚠️ **`pointer-core` 与 `pointer-channels` 是纯 lib**（只有 `src/lib.rs`，没有 `src/main.rs`）。`cargo run -p pointer-core` 跑不起来 —— 它们只能被 `pointer-server`、`pointer-app` 或测试链接。想跑起来请用 `npm run server:dev` / `npm run tauri:dev`，命令清单见 [cli.md](cli.md)。

### 非 Rust 目录

| 路径 | 职责 |
| --- | --- |
| `src/` | Vue 界面，桌面与 Web 共用 |
| `skills/` | 随安装包分发的系统 Skill |
| `docs/` | 文档源；站点工程在 `docs-site/`（[文档站规则](../contributing/docs-site.md)） |
| `scripts/` | 构建、打包、版本同步与文档校验脚本 |

一次对话：界面发消息 → 适配层进入 `pointer-core` 编排 → Provider 流式返回 → 工具/子 Agent 按注册表执行 → 结果写回会话库并推到界面。

## 架构全景

![Pointer 架构全景：桌面端、Web 端与 IM 通道共用同一套 pointer-core，界面只负责交互，编排、工具、Skills、会话都在核心层](/pointer-architecture.zh-CN.svg)

[English 版](/pointer-architecture.en.svg) · 源文件 [`design/pointer-architecture.zh-CN.svg`](../design/pointer-architecture.zh-CN.svg)

实现细节见 [../internals/](../internals/README.md)。两种构建的默认云地址见 [../zh-CN/deploy/editions.md](../deploy/editions.md)。
