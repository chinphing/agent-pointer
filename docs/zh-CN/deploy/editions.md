# 打包口味 × 运行形态：四格构建与部署

[English](../../en/deploy/editions.md) | 简体中文

同一份源码，**打包口味**（`POINTER_EDITION`）与**运行形态**（客户端 / 服务端）正交。本文给出四种组合各自的**构建命令 / 需要的变量 / 产物位置 / 怎么验证 / 外部依赖**，打包维护者照着执行即可。

> 账户与控制面的改造设计（P0/P1）见 [`../../design/control-plane-and-editions.md`](../../design/control-plane-and-editions.md)；跨平台环境准备见 [`../../contributing/cross-platform-build.md`](../../contributing/cross-platform-build.md)。

---

## 0. 术语

| 术语 | 含义 |
| --- | --- |
| 口味 **`managed`（集中管理）** | 构建期注入控制面域名：默认连控制面（登录、目录、用量、自动更新）。 |
| 口味 **standalone（独立）** | **未设置** `POINTER_EDITION`。源码不写入任何控制面域名 ⇒ 未绑定：本地模型 / Key；服务端用账密或 `?sso=`。 |
| **客户端** | Tauri 桌面 App（`src-tauri/`）。Agent 跑在本机进程里，不依赖 pointer-server。 |
| **服务端** | `pointer-server`（axum HTTP/SSE + 同一套 Vue 界面）。浏览器 / 云主机用。 |

四格：

| | 客户端（Tauri） | 服务端（pointer-server） |
| --- | --- | --- |
| **managed** | 官方客户端 / 企业内网客户端 | 官方云主机镜像 / 企业控制面 server |
| **standalone** | 本地客户端（默认） | 自建 server（账密登录） |

---

## 1. 口味与绑定（四格共同前提）

口味只决定**构建期默认值**，不是运行时开关；运行时是否连得上控制面由 `platform_endpoints::control_plane_bound()` / `deployment_mode::is_standalone()` 判定（见 `crates/pointer-core/src/edition.rs` 模块注释）。

| 变量 | 作用 |
| --- | --- |
| `POINTER_EDITION=managed` | 打包口味，编译期烧入。**缺控制面域名时 `crates/pointer-core/build.rs` 直接 panic，构建失败** |
| `VITE_POINTER_EDITION=managed` | 前端口味（`src/lib/platformUrls.ts` → `isManagedEdition()`） |
| `POINTER_API_BASE` | 控制面 API 域名（构建期烧入 `POINTER_BUILTIN_API_BASE`） |
| `POINTER_WEB_BASE` / `VITE_POINTER_WEB_BASE` | 控制面 Web 域名（充值 / 云主机跳转） |
| `COMPUTER_ANNOTATE_API_BASE` | SOM 标注服务域名（构建期烧入） |
| `POINTER_DOWNLOAD_URL` / `VITE_POINTER_DOWNLOAD_URL` | 下载页链接（关于页） |
| `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PATH` | updater 签名私钥，**仅 managed 客户端需要** |

未设置口味时，源码不写入任何控制面默认值 ⇒ 未绑定 ⇒ standalone。

**加载顺序（容易踩）**

- `npm run tauri:dev` / `tauri:build` / Vite（`web:dev`）会自动读仓库根目录 gitignore 的 `pointer.local.env`（**已存在的 OS / CI 环境变量优先**，文件只补空项）。模板见 [`pointer.local.env.example`](../../../pointer.local.env.example)。
- `npm run server:build`（`scripts/build-server.mjs`）**同样读** `pointer.local.env`，并把同一份变量透传给 Vue 构建与 `cargo`（两边不会再各读各的）。构建后、打包前还会校验两半是否一致：managed 口味下二进制缺控制面域名、或 Web 资源缺 Web base，直接非零退出（见 §4.2 需要的变量）。
- 运行时仍可用同名 `POINTER_*` 环境变量覆盖构建期默认值；服务端另有 `POINTER_DEPLOYMENT_MODE`。

---

## 2. 四格总览

| 格子 | 构建命令 | 关键变量 | 产物 | 一句话验证 | 外部依赖 |
| --- | --- | --- | --- | --- | --- |
| [managed × 客户端](#managed-client) | `npm run tauri:build`（或 `build:windows` / `build:macos` / `build:linux`） | `POINTER_EDITION=managed` + 3 个域名 + updater 私钥 | `src-tauri/target/release/bundle/**` + `*.sig` | 构建日志 `[tauri-build] POINTER_EDITION=managed`；bundle 内有 `*.sig` | 控制面域名、updater 私钥 |
| [managed × 服务端](#managed-server) | `npm run server:build`（`pointer.local.env` 写 `managed` + 3 域名，或显式导出） | 同上（不含 updater 私钥）+ `[pointer]` 运行期配置 | `target/release/pointer-server-bundle/pointer-server-{平台}-{架构}.zip`（Linux 另有 `.deb`） | 构建日志 `[server-build] edition=managed domains=3/3 baked, web=managed`；启动日志 `deployment_mode: platform (control_plane_bound=true)` | 控制面 OAuth（secret 与控制面对齐） |
| [standalone × 客户端](#standalone-client) | `npm run tauri:build` | 无（不设口味） | 同上目录，**没有** `*.sig` | 构建日志 `[tauri-build] standalone build (no updater artifacts)` | 无 |
| [standalone × 服务端](#standalone-server) | `npm run server:build` | `[deployment] mode = "standalone"` + 账密 | 同上 zip / deb | `/api/auth/mode` 返回 `standalone`，账密登录成功 | 无（官方签名包需 License） |

---

<a id="managed-client"></a>
## 3. managed × 客户端

### 3.1 构建命令

```bash
cd agent-pointer
npm install
npm run icons                     # 首次或更换 icon.png 后

# 方式 A：本机 pointer.local.env 写好 managed + 域名（日常）
npm run tauri:build

# 方式 B：一次性导出（CI / 临时；键名与 pointer.local.env.example 一致）
POINTER_EDITION=managed VITE_POINTER_EDITION=managed \
  POINTER_API_BASE=https://pointer-api.example.com \
  POINTER_WEB_BASE=https://pointer.example.com \
  COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com \
  VITE_POINTER_WEB_BASE=https://pointer.example.com \
  POINTER_DOWNLOAD_URL=https://pointer.example.com/download \
  TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/pointer-updater.key \
  npm run tauri:build
```

平台快捷脚本（都走同一个 `scripts/tauri-build.mjs`）：`build:windows` / `build:macos`（Universal 签名版 `build:macos:signed`）/ `build:linux`。

首次生成 updater 密钥（只做一次，私钥离线保管）：

```bash
npm run tauri signer generate -w ~/.tauri/pointer-updater.key
```

`src-tauri/tauri.conf.json` 的 `bundle.createUpdaterArtifacts` 为 `true`（只有 standalone 用的 `tauri.personal.conf.json` 关掉），因此 managed 客户端**必须**提供签名私钥，否则 updater 产物签名阶段失败。`scripts/tauri-build.mjs` 会把 `TAURI_SIGNING_PRIVATE_KEY_PATH` 的内容读进 `TAURI_SIGNING_PRIVATE_KEY`。

### 3.2 需要的变量

| 变量 | 必需 | 说明 |
| --- | --- | --- |
| `POINTER_EDITION=managed` | ✅ | 口味。**只接受 `managed` 或留空**：其他任何值（含旧值 `official`）会让构建直接失败，避免静默降级为未绑定 |
| `VITE_POINTER_EDITION=managed` | ✅ | 前端口味；未设时脚本会由 `POINTER_EDITION` 补齐 |
| `POINTER_API_BASE` / `POINTER_WEB_BASE` / `COMPUTER_ANNOTATE_API_BASE` | ✅ | 三个控制面域名，缺一即构建 panic |
| `VITE_POINTER_WEB_BASE` / `POINTER_DOWNLOAD_URL` | 建议 | 前端跳转与关于页下载链接 |
| `TAURI_SIGNING_PRIVATE_KEY` 或 `…_PATH` | ✅ | updater 签名私钥 |

### 3.3 产物位置

```text
src-tauri/target/release/bundle/
├── msi/  nsis/          # Windows 安装包
├── macos/  dmg/         # macOS（.app / .dmg）
├── deb/  appimage/      # Linux
└── *.sig + updater 压缩包（Windows `.nsis.zip`、macOS `.app.tar.gz`、Linux `.AppImage.tar.gz`）
```

- 签名 Universal 打包的产物在 `target/universal-apple-darwin/release/bundle/`。
- 查 updater 产物：`find src-tauri/target/release/bundle -name '*.sig'`（以实际产物为准）。

### 3.4 怎么验证

1. 构建日志出现 `[tauri-build] POINTER_EDITION=managed`。
2. `find src-tauri/target/release/bundle -name '*.sig'` 非空（updater 产物已签名）。
3. 域名已烧入：`strings <release 二进制> | grep -m1 <你的 api_base 域名>`（macOS 打包后为 `Pointer.app/Contents/MacOS/Pointer`）。
4. `RUST_LOG=info` 启动 App，日志出现 `edition: managed`；未登录时发送消息提示「请先登录 Pointer 账户」（已绑定控制面且无身份）。
5. 关于页「下载」链接指向 `POINTER_DOWNLOAD_URL`。

### 3.5 外部依赖

- 控制面 API / Web / SOM 三个域名可达（企业用内网域名）。
- updater 私钥（公钥已在 `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`，**换密钥需同步改这里**）。
- 发版：安装包 + updater 压缩包 + `.sig` 走控制台「客户端发布」。

---

<a id="managed-server"></a>
## 4. managed × 服务端

> 现状：`scripts/build-server.mjs` 读 `pointer.local.env`（已存在的 OS / CI 变量优先），把同一份环境透传给 Vue 构建与 `cargo`；`crates/pointer-core/build.rs` 把域名编译期烧进二进制。构建后、打包前由 `scripts/lib/verify-baked-edition.mjs` 校验两半一致：managed 口味下任一半缺失即构建失败。

### 4.1 构建命令

```bash
cd agent-pointer
npm install

# 方式 A：本机 pointer.local.env 写好 managed + 3 个域名（日常）
npm run server:build

# 方式 B：一次性导出（CI / 临时；键名与 pointer.local.env.example 一致）
POINTER_EDITION=managed \
VITE_POINTER_EDITION=managed \
POINTER_API_BASE=https://pointer-api.example.com \
POINTER_WEB_BASE=https://pointer.example.com \
COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com \
VITE_POINTER_WEB_BASE=https://pointer.example.com \
npm run server:build
```

`server:build` = Vue 同源构建（`VITE_WEB_API_BASE` 置空）→ `cargo build -p pointer-server --release` → **校验两半口味一致** → 打 zip；Linux 上有 `dpkg-deb` 时再加 `.deb`。`--package-only` / `--deb-only` 不重新编译，但同样校验已有产物。

### 4.2 需要的变量

**构建期**（上表 3 个域名 + 口味；写进 `pointer.local.env` 或显式导出）：

| 变量 | 必需 | 说明 |
| --- | --- | --- |
| `POINTER_EDITION=managed` | ✅ | 口味；也是「缺域名就 panic」的开关 |
| `VITE_POINTER_EDITION=managed` | 建议 | 前端口味（云主机 / 充值入口可见性） |
| `POINTER_API_BASE` / `POINTER_WEB_BASE` / `COMPUTER_ANNOTATE_API_BASE` | ✅ | 三个控制面域名 |
| `VITE_POINTER_WEB_BASE` | 建议 | 前端跳转控制面 Web |

**运行期**（`pointer-server.toml`，见 `server/pointer-server.toml.example`）：

| 配置 | 说明 |
| --- | --- |
| `[pointer].api_base` | 控制面 API（环境变量 `POINTER_API_BASE` 优先）。留空则退回构建期烧入值 |
| `[pointer].oauth_client_secret` | 必须与控制面 `THIRD_PARTY_OAUTH_EXCHANGE_SECRET` 一致（**不是** JWT_SECRET） |
| `[server].public_url` | 浏览器可达根 URL；OAuth 回调 `{public_url}/api/auth/oauth/callback` 需登记到控制面 `POINTER_APP_REDIRECT_URIS` |
| `[deployment].mode` | 留空即可：已绑定控制面 ⇒ 自动 `platform`；也可显式写 `platform` |
| `[license]` | **不需要**：platform 模式跳过 License 校验（日志 `license: skipped (platform deployment mode)`） |

### 4.3 产物位置

```text
target/release/pointer-server-bundle/pointer-server-{macos-arm64|macos-x64|windows-x64|linux-x64}.zip
target/release/bundle/deb/pointer-server_0.1.0_{amd64|arm64}.deb      # Linux
```

包内结构（二进制 + `dist/` + `skills/` + `pointer-server.toml.example` + start/stop 脚本）见 [`../../internals/standalone-server-deployment.md`](../../internals/standalone-server-deployment.md) 第 2 节。

### 4.4 怎么验证

```bash
# 0) 构建日志先看守卫结论（两半一致才会继续打包）
#    [server-build] edition=managed domains=3/3 baked, web=managed

# 1) 域名已烧入
strings target/release/pointer-server | grep -m1 <你的 api_base 域名>

# 2) 启动（首次先 cp pointer-server.toml.example pointer-server.toml 并填 [pointer]）
./target/release/pointer-server
#    日志期望：deployment_mode: platform (control_plane_bound=true)

# 3) 认证模式（不再是本地账密）
curl -s http://127.0.0.1:8787/api/auth/mode      # → {"mode":"platform"}

# 4) 浏览器打开 public_url → 跳控制面 OAuth 登录
```

### 4.5 外部依赖

- 控制面（Pointer 官网或企业自建控制面）API 可达，OAuth secret 对齐，回调 URL 已登记。
- 控制面侧的组织 / 计费 / 店铺（本仓库不打）。
- 不需要 License 签发。

---

<a id="standalone-client"></a>
## 5. standalone × 客户端

### 5.1 构建命令

```bash
cd agent-pointer
npm install
npm run icons
npm run tauri:build          # 不设任何 POINTER_* 变量
```

没有 `pointer.local.env`（或文件里没写域名 / 口味）即为本口味。`scripts/tauri-build.mjs` 会自动追加 `--config src-tauri/tauri.personal.conf.json`（关 updater 产物）。

### 5.2 需要的变量

无必需变量。可选 `POINTER_USAGE_REPORT_ENABLED=false`（未绑定时默认即关）。

### 5.3 产物位置

`src-tauri/target/release/bundle/{msi,nsis,macos,dmg,deb,appimage}`，**没有** `*.sig` / updater 压缩包。

### 5.4 怎么验证

1. 构建日志出现 `[tauri-build] standalone build (no updater artifacts)`。
2. `find src-tauri/target/release/bundle -name '*.sig'` 为空。
3. 启动后不登录：设置 → 模型配置 填 API Key 即可对话；云主机页可打开但不可购买。
4. `RUST_LOG=info` 日志出现 `edition: unset (standalone defaults)`。

### 5.5 外部依赖

无。

---

<a id="standalone-server"></a>
## 6. standalone × 服务端

### 6.1 构建命令

```bash
cd agent-pointer
npm install
npm run server:build
```

完整交付流程（构建 → License 签发 → 客户上线 → 验收 → 排障）见 [`../../internals/standalone-server-deployment.md`](../../internals/standalone-server-deployment.md)；配置与实现参考 [`../../developer/standalone-deployment.md`](../../developer/standalone-deployment.md)。

### 6.2 需要的变量

| 配置 | 说明 |
| --- | --- |
| `[deployment] mode = "standalone"`（或 `POINTER_DEPLOYMENT_MODE=standalone`） | deb 的 systemd 单元已内置 |
| `[auth.local]` | `username` / `hmac_secret` / `password_hmac`（用 `pointer-server --hash-password` 生成） |
| `[license].key` | **仅 managed 口味的 standalone 服务端强制**（见下） |
| `[server].public_url` | 反代后的 HTTPS 域名，影响 OAuth 回调与 IM 大文件下载链接 |
| `[usage] report_enabled` | 默认 `false` |

License 规则（`crates/pointer-core/src/license/verify.rs`）：

| 组合 | 启动校验 |
| --- | --- |
| managed + standalone | **强制**：缺失 / 过期 / 绑错机器直接启动失败 |
| managed + platform | 跳过（`license: skipped (platform deployment mode)`） |
| 未设置口味（自建） | 不强制：以 `notConfigured` 运行，许可功能关闭 |

### 6.3 产物位置

同 [4.3](#managed-server)：`target/release/pointer-server-bundle/pointer-server-{平台}-{架构}.zip`（Linux 另有 `.deb`）。

### 6.4 怎么验证

```bash
curl -s http://127.0.0.1:8787/api/auth/mode       # → {"mode":"standalone"}
curl -s http://127.0.0.1:8787/api/license/status  # 自建 notConfigured；官方签名包 valid
```

浏览器打开 `public_url` → 账号密码 + 验证码登录 → 发送对话有回复（先在设置 → 模型配置填 API Key）。完整验收清单见 internals 文档第 8 节。

### 6.5 外部依赖

无。官方签名包需要 Pointer 签发方提供 License（`npm run license-gen:build`）。

---

## 7. CI 与发布口径

- `.github/workflows/release.yml` **只产 standalone 客户端包**：三端都带 `--config src-tauri/tauri.personal.conf.json`，不注入任何 `POINTER_*`，产物上传 Draft Release。**managed（官方 / 企业）客户端在本地或企业 CI 打**。
- 没有 managed 服务端的 CI：managed server 需在带控制面变量的机器 / 流水线上打（照 [4.1](#managed-server) 抄）。`scripts/build-server.mjs` 会把 `pointer.local.env` 或 CI 环境变量同时交给两半，并在打包前校验；CI 上请用环境变量（没有 `pointer.local.env` 文件）。
- 发布 managed 客户端后，安装包 + updater 压缩包 + `.sig` 走控制台「客户端发布」；standalone 包直接发 GitHub Release。

---

## 8. 日常开发

```bash
cp pointer.local.env.example pointer.local.env    # 需要绑控制面时才建
npm run tauri:dev                                 # 自动读 pointer.local.env；无文件 ⇒ standalone
npm run server:dev                                # 服务端调试（不受 pointer.local.env 影响）
```

| 角色 | 建议 |
| --- | --- |
| 开源 / 个人 | 不建文件（不设域名即未绑定 ⇒ standalone） |
| 官网维护者 | 在 `pointer.local.env` 写 `managed` + 官网 API/Web/SOM 域名 |
| 企业内部 | 同样写 `managed`，域名改成内网控制面 |

不要把生产域名、签名口令写进公开 `main`。需要联调控制面时只改本机 `pointer.local.env`。

加载实现：`scripts/lib/load-pointer-local-env.mjs`（由 `tauri-dev.mjs` / `tauri-build.mjs` / `build-server.mjs` / `vite.config.ts` 调用）。`POINTER_*` 与对应 `VITE_*` 会互相补齐空白项。服务端两半一致性校验：`scripts/lib/verify-baked-edition.mjs`（`build-server.mjs` 在打包前调用）。

---

## 9. 维护流程

- 通用功能：公开仓库 PR → 本地包与绑定控制面的包共用。
- 只对付费云能力：改私有云 / 计费 / 更新服务。
- 换域名、更新公钥、证书：只改官方 CI 或维护者本机 `pointer.local.env`，勿提交。
- 外部 PR 默认按未绑定理解；合入前确认没有写死某一家控制面域名。

---

## 10. 发布前检查

- 签名与密钥只放在 CI 的 secret / environment 里，不写进仓库、不写进本机配置文件。
- 在 GitHub 打开 Private vulnerability reporting。
- managed 包发布前确认：`*.sig` 齐全、域名指向正确环境（生产 / 预发不混）、`plugins.updater.pubkey` 与签名私钥配对。
- managed 服务端发布前确认：`npm run server:build` 日志为 `edition=managed domains=3/3 baked, web=managed`（守卫不通过会直接中断打包）。
