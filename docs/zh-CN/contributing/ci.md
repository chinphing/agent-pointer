# CI 与发布流程

[English](../../en/contributing/ci.md) | 简体中文

仓库有 **4 支 workflow**（`.github/workflows/`）。这页说明每支做什么、怎么在本地复现它的检查，以及提 PR 前该跑什么。

```
ci.yml          ← 主 CI：测试 + 审计 + 清单一致性 + 密钥扫描（PR 与 main 都跑）
docs-links.yml  ← 文档相对链接与锚点校验
docs-site.yml   ← 构建文档站并发布到 GitHub Pages
release.yml     ← 打 tag 后三端打包桌面客户端（只产 standalone）
```

## 四支 workflow 一览

| 文件 | 名字 | 触发 | 作用 |
|------|------|------|------|
| `ci.yml` | CI | PR、push `main`、手动 | 前端与 Rust 测试、依赖审计、第三方清单、gitleaks |
| `docs-links.yml` | Docs links | PR、push `main` | `node scripts/check-doc-links.mjs` |
| `docs-site.yml` | Docs site | push `main`（改到 `docs/**` / `docs-site/**`）、手动 | 构建文档站 → GitHub Pages |
| `release.yml` | Release (standalone) | push tag `v*.*.*`、手动 | 三端打包桌面客户端 → Draft Release |

## `ci.yml`：唯一的主 CI

单 job（`test`）跑在 `ubuntu-24.04`，**没有** `concurrency`、也没有 `timeout-minutes`。

| 步骤 | 命令 |
|------|------|
| Checkout | `actions/checkout@v4` |
| 腾磁盘空间 | `sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc …`、`docker system prune -af` |
| Node | `actions/setup-node@v4`，**`node-version: 20`** |
| Rust | `dtolnay/rust-toolchain@stable` |
| 装 Linux 系统依赖 | `apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev …` |
| 装前端依赖 | **`npm ci`** |
| 前端测试 | `npm test` |
| Rust 测试 | `cargo test --workspace` |
| Rust 依赖审计 | `cargo install cargo-audit --locked` + `cargo audit` |
| npm 依赖审计 | `npm audit --audit-level=high` |
| 第三方清单一致性 | `npm run licenses` + `git diff --exit-code -- THIRD-PARTY-NOTICES.md` |
| 密钥扫描 | `gitleaks/gitleaks-action@v2` |

> 「腾磁盘空间」是 CI runner 专用步骤，**不要在本机跑** —— 它会删系统目录。

### 本地复现

```bash
npm ci
npm test
cargo test --workspace
cargo install cargo-audit --locked   # 首次需要
cargo audit
npm audit --audit-level=high
npm run licenses && git diff --exit-code -- THIRD-PARTY-NOTICES.md
```

三条容易踩的：

1. **`THIRD-PARTY-NOTICES.md` 必须和依赖一致** —— 改完依赖要跑 `npm run licenses` 并把生成的清单一起提交；否则 CI 卡在 `git diff --exit-code`。
2. **`cargo audit` 读 `.cargo/audit.toml`**，里面有两条例外（quick-xml / ossify 链路），带 `REVISIT BY: 2027-01-01`。新增依赖被审计拦下时，先看是不是这里。
3. **Node 版本是 20**，硬编码在 workflow 里（仓库没有 `.nvmrc`）。本地 Node 版本不一致时先对齐再排查。

gitleaks 用官方 action，走 checkout 的默认深度；要本地自查可装 `gitleaks` 后跑 `gitleaks detect`。

## `docs-links.yml`

**零依赖**：不装 npm 包、不构建，直接跑

```bash
node scripts/check-doc-links.mjs
```

它检查仓库里所有 Markdown 的**相对链接与锚点**（跳过代码块与行内代码），并按 `docs-site/site-map.mjs` 的**已发布页面集合**解析 `/route` 形式的链接 —— 所以指向「不上站」页面的路由链接会被判为断链。规则细节见 [文档站规则](docs-site.md)。

## `docs-site.yml`

- **触发**：push `main` 且改动落在 `docs/**`、`docs-site/**` 或这支 workflow 自身；也支持手动触发
- **权限**：`contents: read` + `pages: write` + `id-token: write`（OIDC，不需要任何 repo secret）
- **并发**：`group: pages`，不取消进行中的发布
- **步骤**：

```bash
npm ci                      # working-directory: docs-site
npm run docs:build          # 仓库根执行 → npm --prefix docs-site run build
# 上传 docs-site/.vitepress/dist → actions/deploy-pages
```

> 首次启用需要在 GitHub 仓库的 *Settings → Pages* 把 Source 选成 **GitHub Actions**。

## `release.yml`

**只产 standalone 客户端**：三端都带 `--config src-tauri/tauri.personal.conf.json`，不注入控制面域名，也没有 updater 产物。managed（官方 / 企业）包要在本地或企业 CI 打，见 [editions.md](../deploy/editions.md)。

| Runner | 额外参数 | 产物 |
|--------|----------|------|
| `windows-latest` | — | Windows MSI |
| `macos-latest` | `--target universal-apple-darwin` | Universal macOS |
| `ubuntu-24.04` | — | Linux deb + AppImage |

步骤：checkout → Node 20 → Rust stable（macOS 加两个 target）→（Linux 装系统依赖）→ **`npm install`** → `npm run icons` → `tauri-apps/tauri-action` → 建 **Draft** Release。

- ⚠️ 这里用的是 `npm install` 而不是 `npm ci`（与 `ci.yml` 不同），不要照抄到别处
- Release 是 **draft**，需要人工确认后发布
- macOS 签名相关 secrets：`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_TEAM_ID`、`APPLE_ID`、`APPLE_PASSWORD`（只在 macOS 分支注入）
- 各平台的环境准备与产物细节见 [跨平台开发与打包](cross-platform-build.md)

## 版本：人工门禁，CI 不兜底

**workflow 里没有 `VERSION`、`version:sync`、`version:check` 任何一步。** 版本是人工流程：

```bash
# 1) 只改根目录 VERSION
# 2) 同步到各处的字面量
npm run version:sync
# 3) 确认无漂移（漂移时退出码 1）
npm run version:check
# 4) 提交后再打 tag，触发 release.yml
git tag v0.1.3 && git push origin v0.1.3
```

> ⚠️ **`version:check` 没有挂进 CI。** 忘记跑 `version:sync` 时，CI 不会报错，只有本地 `version:check`（或人工 review）能发现漂移。同理，release workflow 也**不校验 tag 与 `VERSION` 是否一致**。
>
> 同步会写到哪些文件、哪些地方要用派生值，见 [版本号管理](versioning.md)。

## PR 前自检清单

- [ ] `npm test` —— 前端单测
- [ ] `cargo test --workspace`（或只跑你改动的 crate）
- [ ] 改了依赖 → `npm run licenses`，并把 `THIRD-PARTY-NOTICES.md` 一起提交
- [ ] 改了文档 → `node scripts/check-doc-links.mjs`（0 断链）
- [ ] 改了 `VERSION` → `npm run version:sync` + `npm run version:check`
- [ ] commit 带 `Signed-off-by`（用 `git commit -s`）
- [ ] PR 描述勾选模板里的 Test plan（`npm test` / `cargo test` / 是否检查了 bound 与 unbound 的默认值）

## DCO：签署来源（约定，非 CI 强制）

`CONTRIBUTING.md` 要求每个 commit 带：

```
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` 会自动加这一行。签署表示你确认该贡献由你提交、可按 Apache-2.0 授权。

**但仓库里没有任何自动化强制** —— 没有 DCO workflow，`.github/` 里只有 PR 模板的一行文字提到它。也就是说漏签不会被 CI 拦下，只能靠 review 发现。

## Dependabot

`.github/dependabot.yml` 只声明两个 ecosystem：`npm` 与 `cargo`，`directory: /`，`schedule.interval: weekly`，两者都设了

```yaml
open-pull-requests-limit: 0
```

含义是**常规版本升级 PR 全部关闭**（噪音大、大版本跳跃容易直接让 CI 变红），**安全更新不受这个限制**，仍会由 Dependabot security updates 发起 PR。所以看到 Dependabot 的 PR 基本就是安全更新，优先处理。

## 相关

- [跨平台开发与打包](cross-platform-build.md) —— 三平台环境、打包产物、发版细节
- [版本号管理](versioning.md) —— `VERSION` 单一来源与同步位置
- [文档站规则](docs-site.md) —— 发布规则与链接校验
- [命令行与脚本参考](../developer/cli.md) —— 上面用到的 `npm run` 脚本逐个说明
