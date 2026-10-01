# 版本号管理

[English](../../en/contributing/versioning.md) | 简体中文

应用版本以仓库根目录 **`VERSION`** 为唯一来源（当前如 `0.1.2`）。

让 AI / Agent 升级版本时：只改 `VERSION`，然后**必须**执行 `npm run version:sync`（见 `.cursor/rules/versioning.mdc`）。不要手改各处的版本字面量。

## 升级步骤

1. 编辑根目录 `VERSION`（只改这一处）
2. 运行同步：

```bash
npm run version:sync
```

3. 确认无漂移：

```bash
npm run version:check
```

## 会同步到的位置

| 目标 | 用途 |
|------|------|
| `package.json` / `package-lock.json` | npm / 前端包版本 |
| `src-tauri/tauri.conf.json` | Tauri 安装包与 `getVersion()`；`build.rs` 注入 `POINTER_APP_VERSION` |
| `Cargo.toml` `[workspace.package].version` | 各 Rust crate 通过 `version.workspace = true` 继承 |
| `src/lib/appVersion.ts` | 前端回退展示（生成文件，勿手改） |

其它引用请走派生值，不要再写死字面量：

- Rust 运行时：`pointer_core::client_env::app_version()`（优先 `POINTER_APP_VERSION`）
- Rust crate：`env!("CARGO_PKG_VERSION")`
- deb 打包：`scripts/build-server-deb.mjs` 读取 `package.json`

设计文档里的示例版本号（如自动更新用例）可保持历史数字，不必每次跟着改。
