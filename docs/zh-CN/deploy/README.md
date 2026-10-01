# 打包与部署：四格清单入口

[English](../../en/deploy/README.md) | 简体中文

同一份源码，三个正交的轴决定你要做什么：

| 轴 | 取值 | 在哪 |
|---|---|---|
| **打包口味** `POINTER_EDITION` | `managed`（集中管理，构建期注入控制面域名）／ **不设置**（standalone，独立） | 本文 §1–§4 |
| **运行形态** | 客户端（Tauri 桌面 App）／ 服务端（`pointer-server`） | 本文 §1–§4 |
| **目标平台** | Windows / macOS / Linux | [§5 平台打包](#platforms) |

**本页是唯一入口。** 先在 §0 定位到你的格子，照 checklist 做完；需要细节再进详细手册。

> **术语**：`official` **不是** `POINTER_EDITION` 的取值，它只用来指 Pointer 官方发布。
> 写成别的值（含 `official`）会让构建**直接失败** —— 这是刻意的，避免静默降级成未绑定。

---

<a id="where"></a>
## 0. 先定位：你要做什么？

| 你要做的事 | 你的格子 | 去哪 |
|---|---|---|
| 自己用，或给同事发一个**自己构建**的客户端 | standalone × 客户端 | [§3](#standalone-client) |
| 自己搭一台服务器给团队用 | standalone × 服务端 | [§4](#standalone-server) |
| 企业：客户端连**内网**控制面 | managed × 客户端 | [§1](#managed-client) |
| 企业：部署内网控制面服务端 | managed × 服务端 | [§2](#managed-server) |
| 想要 Pointer **官方签名包** | —— | 不在本仓产出，见 [`../../user/which-build.md`](../../user/which-build.md) |

**总表**（每格五项：构建命令 / 需要的变量 / 产物位置 / 怎么验证 / 外部依赖）：

| 口味 | 客户端（Tauri 桌面 App） | 服务端（`pointer-server`） |
|------|--------------------------|-----------------------------|
| **`managed`**（集中管理） | [§1](#managed-client) | [§2](#managed-server) |
| **standalone**（独立，默认） | [§3](#standalone-client) | [§4](#standalone-server) |

详细版（含产物清单、排障、CI 口径）：[`editions.md`](editions.md)。

---

<a id="managed-client"></a>
## 1. managed × 客户端

- [ ] **1. 准备变量** —— 在 `pointer.local.env` 写：`POINTER_EDITION=managed`、`VITE_POINTER_EDITION=managed`、三个域名（`POINTER_API_BASE` / `POINTER_WEB_BASE` / `COMPUTER_ANNOTATE_API_BASE`）、updater 签名私钥（`TAURI_SIGNING_PRIVATE_KEY` 或 `TAURI_SIGNING_PRIVATE_KEY_PATH`）
- [ ] **2. 装依赖与图标** —— `npm install && npm run icons`
- [ ] **3. 构建** —— `npm run tauri:build`
- [ ] **4. 验证** —— 构建日志有 `[tauri-build] POINTER_EDITION=managed`；`find src-tauri/target/release/bundle -name '*.sig'` 有结果；启动后关于页显示更新入口
- [ ] **5. 分发** —— 产物在 `src-tauri/target/release/bundle/**`

**外部依赖**：三个控制面域名可达（企业用内网域名）；updater 签名私钥。

**为什么必须有签名私钥**：`src-tauri/tauri.conf.json` 的 `bundle.createUpdaterArtifacts` 为 `true`（只有 standalone 用的 `tauri.personal.conf.json` 关掉），缺私钥会在签名阶段失败。

---

<a id="managed-server"></a>
## 2. managed × 服务端

- [ ] **1. 准备变量** —— `POINTER_EDITION=managed` + `VITE_POINTER_EDITION=managed` + 三个域名。写进 `pointer.local.env` 或显式导出（**OS / CI 环境变量优先**，文件只补空缺）
- [ ] **2. 构建** —— `npm install && npm run server:build`
- [ ] **3. 验证域名已烧入** —— 构建日志有 `[server-build] edition=managed domains=3/3 baked, web=managed`；`strings target/release/pointer-server | grep -m1 <你的 api_base 域名>` 有输出
- [ ] **4. 运行期配置** —— `pointer-server.toml` 的 `[pointer]`：`api_base`、`oauth_client_secret`（与控制面 `THIRD_PARTY_OAUTH_EXCHANGE_SECRET` 对齐）、`public_url`；回调 URL 已在控制面登记
- [ ] **5. 启动并验证** —— 日志 `deployment_mode: platform (control_plane_bound=true)`；`curl -s http://127.0.0.1:8787/api/auth/mode` → `{"mode":"platform"}`；浏览器打开 `public_url` 跳控制面 OAuth 登录
- [ ] **6. 记住** —— platform 模式**跳过 License 校验**，不需要 `[license]`

**外部依赖**：控制面（Pointer 官网或企业自建）API 可达，OAuth secret 对齐，回调 URL 已登记。

---

<a id="standalone-client"></a>
## 3. standalone × 客户端

- [ ] **1. 变量** —— **不需要**。确认没有 `pointer.local.env`，或其中不含口味与域名
- [ ] **2. 装依赖与图标** —— `npm install && npm run icons`
- [ ] **3. 构建** —— `npm run tauri:build`（脚本自动追加 `--config src-tauri/tauri.personal.conf.json`，关掉 updater 产物）
- [ ] **4. 验证** —— 构建日志有 `[tauri-build] standalone build (no updater artifacts)`；bundle 内**没有** `*.sig`；启动日志有 `edition: unset (standalone defaults)`；左下角显示「本地模式」

**外部依赖**：无。装完填自己的模型 API Key 即可用。

---

<a id="standalone-server"></a>
## 4. standalone × 服务端

- [ ] **1. 构建** —— `npm install && npm run server:build`
- [ ] **2. 配置** —— `cp server/pointer-server.toml.example pointer-server.toml`，设 `[deployment] mode = "standalone"` 与账密
- [ ] **3. 启动并验证** —— `curl -s http://127.0.0.1:8787/api/auth/mode` → `{"mode":"standalone"}`；浏览器用账密登录成功；发一条对话有回复（先在设置 → 模型配置填 API Key）
- [ ] **4. License** —— **不强制**：未设置口味以 `notConfigured` 运行，许可功能关闭

**外部依赖**：无。

**完整交付流程**（构建 → License 签发 → 客户上线 → 验收 → 排障）见 [`../../internals/standalone-server-deployment.md`](../../internals/standalone-server-deployment.md)；客户侧运维见 [`../../user/standalone-server.md`](../../user/standalone-server.md)。

---

<a id="platforms"></a>
## 5. 平台打包

三平台的环境前置、构建命令、产物位置与 CI 口径见 [`platforms.md`](platforms.md)（详细正文 [`../../contributing/cross-platform-build.md`](../../contributing/cross-platform-build.md)）。

---

## 6. 相关文档

| 文档 | 内容 |
|---|---|
| [`editions.md`](editions.md) | 四格详细步骤（构建命令 / 变量 / 产物 / 验证 / 外部依赖） |
| [`platforms.md`](platforms.md) | Windows / macOS / Linux 平台打包索引（环境前置 / 产物 / CI 口径） |
| [`../../contributing/cross-platform-build.md`](../../contributing/cross-platform-build.md) | Windows / macOS / Linux 环境与打包命令 |
| [`../../internals/standalone-server-deployment.md`](../../internals/standalone-server-deployment.md) | standalone 服务端完整交付流程（构建 → License → 上线 → 验收） |
| [`../../developer/standalone-deployment.md`](../../developer/standalone-deployment.md) | pointer-server 配置参考（实现向） |
| [`../../user/which-build.md`](../../user/which-build.md) | 官方签名包与本地构建的差别（用户视角） |
| [`../../user/standalone-server.md`](../../user/standalone-server.md) | 自建 pointer-server 运维 |
| [`../../design/control-plane-and-editions.md`](../../design/control-plane-and-editions.md) | 账户与控制面改造设计（背景） |

[返回文档总索引](../../README.md)
