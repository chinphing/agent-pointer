# 贡献者指南
[English](../../en/contributing/README.md) | 简体中文

面向**本仓库贡献者与打包维护**：从源码构建、平台专项 UI、内部评测等。

| 文档 | 说明 |
|------|------|
| [**editions.md**](../deploy/editions.md) | **打包口味 × 运行形态四格**（managed / standalone × 客户端 / 服务端）：构建命令 / 变量 / 产物 / 验证 / 外部依赖（先看 [改造设计](../design/control-plane-and-editions.md)） |
| [**cross-platform-build.md**](cross-platform-build.md) | **Windows / macOS / Linux 开发与打包**（环境、命令、产物、CI） |
| [**versioning.md**](versioning.md) | **版本号单一来源**（`VERSION` + `npm run version:sync`） |
| [**ci.md**](ci.md) | **CI 与发布流程**：四支 workflow、本地复现、PR 前自检、DCO 与 Dependabot |
| [**docs-site.md**](docs-site.md) | **文档站规则**：三层分区、`SITE_EXCLUDES`、侧栏与路由、预览与链接校验 |
| [**macos-window-chrome.md**](macos-window-chrome.md) | **macOS 红绿灯与顶栏对齐**（reapply/repair、紧凑模式、常量同步、排查） |
| [../ui/visual-theme.md](../ui/visual-theme.md) | **界面颜色 token**（灰阶表面 + 系统蓝；禁止 `slate-*` / 硬编码 hex；Diff 除外） |
| [web-media-and-desktop-snapshot.md](web-media-and-desktop-snapshot.md) | Web 媒体与桌面快照 |
| [coder-agent-offline-eval-setup.md](coder-agent-offline-eval-setup.md) | Coder 离线评测环境搭建 |

> `macos-window-chrome.md` 与 `web-media-and-desktop-snapshot.md` 讲的是 **UI 实现**，侧栏里归入 **维护者笔记 → UI 笔记**（归位改配置、不改路径，见 [文档站规则](docs-site.md)）。

本地开发流程见仓库根目录 **[`DEVELOPMENT.md`](../../../DEVELOPMENT.md)**。

用户使用说明见 **[`../user/`](../user/README.md)**；外部集成见 **[`../developer/`](../developer/README.md)**。

[返回文档总索引](../README.md)
