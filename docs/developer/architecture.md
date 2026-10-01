# 架构说明

[English](../en/developer/architecture.md) | 简体中文

桌面端和 Web 端共用一套对话与工具实现。

```text
Vue 界面
├─ 桌面：Tauri Adapter → src-tauri → crates/pointer-core
└─ Web：Web Adapter → server (HTTP/SSE) → crates/pointer-core
```

## 目录

| 路径 | 职责 |
| --- | --- |
| `src/` | Vue 界面，桌面与 Web 共用 |
| `src-tauri/` | 桌面壳、IPC、更新插件 |
| `server/` | Web 与独立部署的 HTTP/SSE |
| `crates/pointer-core` | 模型、工具、Skills、会话、存储 |
| `crates/pointer-channels` | IM 通道 |
| `skills/` | 随安装包分发的系统 Skill |

一次对话：界面发消息 → 适配层进入 `pointer-core` 编排 → Provider 流式返回 → 工具/子 Agent 按注册表执行 → 结果写回会话库并推到界面。

架构全景图：[中文](../design/pointer-architecture.zh-CN.svg) · [English](../design/pointer-architecture.en.svg)。

实现细节见 [../internals/](../internals/README.md)。两种构建的默认云地址见 [../zh-CN/deploy/editions.md](../zh-CN/deploy/editions.md)。
