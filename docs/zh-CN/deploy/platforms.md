# 平台打包索引（Windows / macOS / Linux）

[English](../../en/deploy/platforms.md) | 简体中文

**本文只做索引**：三平台各自的**环境前置 / 构建命令 / 产物位置 / CI 口径**，每节链到详细正文 [`../../contributing/cross-platform-build.md`](../../contributing/cross-platform-build.md)，不复制正文（该文被大量交叉引用，锚点稳定优先）。

> 打包口味 × 运行形态的四格步骤见 [`editions.md`](editions.md)；总入口见 [`README.md`](README.md)。

---

## 1. 环境前置

| 平台 | 额外需要 | 详细正文 |
|---|---|---|
| **Windows** | **Microsoft C++ Build Tools**（勾选「使用 C++ 的桌面开发」）、**WebView2 Runtime**、**LLVM / libclang**（`silk-v3-sys` 走 bindgen；仅**从源码编译**需要，最终用户装 MSI 不需要） | [Windows › 环境要求](../../contributing/cross-platform-build.md#windows) |
| **macOS** | `xcode-select --install`；打包 `.app` / `.dmg` **必须在 macOS 上构建** | [macOS › 环境要求](../../contributing/cross-platform-build.md#macos) |
| **Linux（Ubuntu / Debian）** | 构建基线 **ubuntu-24.04**（PipeWire ≥ 1.0 头文件；产物 glibc ≥ 2.39）；一键装依赖：`bash scripts/install-linux-build-deps.sh` | [Linux › 系统依赖](../../contributing/cross-platform-build.md#linux) |

三平台通用前置：**Node.js 20+**、**Rust stable**、本机 C 编译器（编译 `sqlite-cjk-fts` 原生扩展）——见 [通用依赖](../../contributing/cross-platform-build.md#通用依赖)。

---

## 2. 客户端（Tauri 桌面 App）

| 平台 | 命令 | 产物 |
|---|---|---|
| Windows | `npm run icons && npm run build:windows` | `src-tauri/target/release/bundle/msi/*.msi`；`src-tauri/target/release/pointer-app.exe` |
| macOS | `npm run icons && npm run build:macos`（Universal：`build:macos:universal`；签名：`build:macos:signed`） | `src-tauri/target/release/bundle/{macos,dmg}/**`；Universal 在 `src-tauri/target/universal-apple-darwin/release/bundle/**` |
| Linux | `npm run icons && npm run build:linux` | `src-tauri/target/release/bundle/{deb,appimage}/**` |

各平台完整命令、格式取舍与排障见对应小节：[Windows](../../contributing/cross-platform-build.md#windows) · [macOS](../../contributing/cross-platform-build.md#macos) · [Linux](../../contributing/cross-platform-build.md#linux)。签名与分发（Authenticode / Developer ID + 公证 / GPG）见 [签名与分发（正式发布）](../../contributing/cross-platform-build.md#签名与分发正式发布)。

---

## 3. 服务端（`pointer-server`）

| 项 | 内容 |
|---|---|
| 命令 | `npm install && npm run server:build` |
| 产物 | `target/release/pointer-server-bundle/pointer-server-{平台}-{架构}.zip`（Linux 另有 `.deb`） |
| 正文 | [Web 端开发与部署](../../contributing/cross-platform-build.md#web-端开发与部署) · [打包通用流程](../../contributing/cross-platform-build.md#打包通用流程) |

服务端不受平台打包格式限制：**zip 全平台通用**，`.deb` 由 Linux 构建自动附加。四格口味 / 变量 / 运行期配置见 [`editions.md`](editions.md)。

---

## 4. CI 口径

`.github/workflows/release.yml` **只产 standalone 客户端包**：三端都带 `--config src-tauri/tauri.personal.conf.json`，不注入控制面域名，也没有 updater 产物。**managed 客户端需在本地或企业 CI 打**，见 [`editions.md`](editions.md)。

详细步骤（触发方式、Linux 系统依赖、Draft Release）见 [GitHub Actions 三端发版](../../contributing/cross-platform-build.md#github-actions-三端发版)。

---

## 5. 相关文档

| 文档 | 内容 |
|---|---|
| [`README.md`](README.md) | 打包与部署四格清单入口 |
| [`editions.md`](editions.md) | 口味 × 运行形态四格（构建命令 / 变量 / 产物 / 验证 / 外部依赖） |
| [`../../contributing/cross-platform-build.md`](../../contributing/cross-platform-build.md) | 平台环境、本地开发、编译打包与 CI 正文 |
| [`../../internals/standalone-server-deployment.md`](../../internals/standalone-server-deployment.md) | standalone 服务端完整交付流程 |

[返回文档总索引](../../README.md)
