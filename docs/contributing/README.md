# 贡献者指南（contributing）

面向**本仓库贡献者与打包维护**：从源码构建、平台专项 UI、内部评测等。

| 文档 | 说明 |
|------|------|
| [**editions.md**](editions.md) | **打包口味 × 运行形态四格**（managed / standalone × 客户端 / 服务端）：构建命令 / 变量 / 产物 / 验证 / 外部依赖（先看 [改造设计](../design/control-plane-and-editions.md)） |
| [**cross-platform-build.md**](cross-platform-build.md) | **Windows / macOS / Linux 开发与打包**（环境、命令、产物、CI） |
| [**versioning.md**](versioning.md) | **版本号单一来源**（`VERSION` + `npm run version:sync`） |
| [**macos-window-chrome.md**](macos-window-chrome.md) | **macOS 红绿灯与顶栏对齐**（reapply/repair、紧凑模式、常量同步、排查） |
| [../ui/visual-theme.md](../ui/visual-theme.md) | **界面颜色 token**（灰阶表面 + 系统蓝；禁止 `slate-*` / 硬编码 hex；Diff 除外） |
| [web-media-and-desktop-snapshot.md](web-media-and-desktop-snapshot.md) | Web 媒体与桌面快照 |
| [coder-agent-offline-eval-setup.md](coder-agent-offline-eval-setup.md) | Coder 离线评测环境搭建 |

本地开发流程见仓库根目录 **[`DEVELOPMENT.md`](../../DEVELOPMENT.md)**。

用户使用说明见 **[`../user/`](../user/README.md)**；外部集成见 **[`../developer/`](../developer/README.md)**。

[返回文档总索引](../README.md)
