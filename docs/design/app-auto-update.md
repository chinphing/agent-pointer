# 桌面客户端自动升级设计

> Pointer 桌面端（Tauri 2）自动升级方案
>
> **更新策略（已确认）：** 后台检查 → 后台下载 → 重启前确认（类似 VS Code / Cursor）
>
> **范围：** 仅 Tauri 桌面端；Web 端（pointer-server）不涉及
>
> **依赖：** 官网发版体系（`pointer-official` 控制台「客户端发布」+ 公开 API）
>
> **原则：** 版本源、下载源、发布动作均与官网 `/download` 页完全一致，共用同一套 API 与存储。

---

## 1. 背景与目标

### 1.1 现状

| 环节 | 状态 |
|------|------|
| 三端打包（MSI / DMG / deb / AppImage） | ✅ |
| 后台上传 + 发布 + 官网下载页 | ✅ |
| 登录时上报 `app_version` | ✅ |
| 客户端检查/下载/安装更新 | ❌ |

当前用户需手动访问下载页重装。本设计在现有发布体系上增加 **updater 产物** 与 **Tauri updater 插件**，实现无感下载、可控重启。

### 1.2 目标

- 启动后静默检查新版本，后台下载，不打断对话
- 下载完成后轻量提示，用户选择「立即重启」或「稍后」
- 设置页提供「检查更新」与当前版本展示
- **完全复用官网发版机制**：与 `/download` 页共用同一套上传、发布、存储、下载 API
- 客户端更新检查与 OAuth 登录使用同一 API 域名（`platform_endpoints::api_base()`）
- macOS / Windows / Linux（AppImage）三端支持；Linux deb 保持手动升级

### 1.3 非目标（首版）

- 增量/delta 更新
- 多 channel（beta / stable）灰度
- 强制升级（`min_version`）—— 预留字段，二期实现
- Web 端自动刷新
- pointer-zero 等其他产品

---

## 2. 方案选型

采用 **Tauri 官方 `tauri-plugin-updater` + 扩展官网现有发版 API**。

| 方案 | 结论 |
|------|------|
| Tauri updater + 官网 `pointer-official` API | ✅ 选用 |
| 仅版本检查 + 打开官网下载页 | ❌ 体验差，作降级兜底 |

### 2.1 与官网发版机制的关系

官网现有发版链路（见 `pointer-official/docs/app-releases-deployment.md`）：

```
本地/CI 构建安装包
  → 控制台「后台管理 → 客户端发布」上传
  → 点击「发布」
  → GET /api/downloads 驱动 /download 页
```

自动升级 **在同一条链路上扩展**，不新增独立发布入口：

```
本地/CI 构建（安装包 + updater 产物 + .sig）
  → 同一控制台、同一 version 上传（含 updater 槽位）
  → 同一次「发布」
  → GET /api/downloads        → 官网手动下载（不变）
  → GET /api/updates/latest   → 客户端自动升级（新增）
  → GET /api/downloads/files/{id} → 两种场景共用文件下载（不变）
```

**单一真相源：** 数据库 `app_release_artifacts` 中 **已发布（published）** 的版本。官网下载页与客户端 updater 永远读到同一版本号、同一存储文件。

---

## 3. 架构

```
┌──────────────────────────────────────────────────────────────┐
│  本地构建（npm run build:*）                                  │
│  tauri build (createUpdaterArtifacts: true)                  │
│  ├─ 官网安装包: MSI, DMG, deb, AppImage                       │
│  └─ Updater 产物: .app.tar.gz/.sig, .msi/.sig, AppImage/.sig │
└───────────────────────────┬──────────────────────────────────┘
                            │ POST /admin/app-releases/upload
                            ▼
┌──────────────────────────────────────────────────────────────┐
│  官网 pointer-official（控制台 + API + 共享存储）              │
│  APP_RELEASES_STORAGE_DIR                                    │
│  app_release_artifacts (+ signature)                         │
│  POST .../publish  ← 一次发布，两端同时生效                    │
├──────────────────────────────────────────────────────────────┤
│  GET /api/downloads          → 官网 /download 页               │
│  GET /api/updates/latest     → 桌面端 updater manifest       │
│  GET /api/downloads/files/{id} → 安装包/updater 二进制下载    │
└───────────────────────────┬──────────────────────────────────┘
                            │ HTTPS（POINTER_API_BASE 同源）
                            ▼
┌──────────────────────────────────────────────────────────────┐
│  pointer-app 桌面端（platform 模式）                          │
│  tauri-plugin-updater                                        │
│  endpoint = {api_base}/api/updates/latest?...                │
│  ├─ 启动 +30s / 每 6h / 手动 → check()                       │
│  ├─ 有更新 → 从 /api/downloads/files/{id} 后台下载           │
│  └─ 完成 → 提示重启 → relaunch()                              │
└──────────────────────────────────────────────────────────────┘

Standalone 部署：无官网 API，不启用自动升级。
```

---

## 4. 密钥与签名

Tauri updater 使用 **独立的 minisign Ed25519 密钥对**，与 Apple Developer ID / Windows 代码签名证书无关。

| 密钥 | 用途 | 存放 |
|------|------|------|
| Public key | 写入 `tauri.conf.json` → `plugins.updater.pubkey` | 可公开 |
| Private key | 构建时签名 updater 产物 | 构建环境变量 + 安全备份 |

生成（一次性）：

```bash
cd pointer-app
npm run tauri signer generate -w ~/.tauri/pointer-updater.key
```

构建环境变量：

```bash
export TAURI_SIGNING_PRIVATE_KEY="~/.tauri/pointer-updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="..."
```

**风险：** 私钥丢失后，已安装用户无法验证新更新。必须备份并纳入密钥管理流程。

---

## 5. 构建产物

### 5.1 tauri.conf.json 变更

```json
{
  "bundle": {
    "createUpdaterArtifacts": true,
    "targets": ["msi", "nsis", "deb", "appimage", "dmg"]
  },
  "plugins": {
    "updater": {
      "pubkey": "<PUBLIC_KEY_CONTENT>",
      "endpoints": [
        "https://pointer-api.readflowai.com/api/updates/latest?target={{target}}&arch={{arch}}&current_version={{current_version}}"
      ],
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

说明：

- Windows 新增 **NSIS** target，供 updater 静默安装（`passive` 模式）；MSI 仍供官网下载
- `createUpdaterArtifacts: true` 为 Tauri v2 推荐值（非 `v1Compatible`）
- **endpoint 域名** 与 `platform_endpoints::DEFAULT_API_BASE` / `POINTER_API_BASE` 一致；构建时可通过 `build.rs` 或环境变量注入，避免与 OAuth 域名分叉

### 5.2 各平台产物对照

| 平台 | 官网下载（现有 kind） | Updater kind（新增） | Updater 文件 |
|------|----------------------|---------------------|--------------|
| Windows | `windows` → `.msi` | `windows_updater` | `.msi` + `.msi.sig` 或 NSIS `.exe` + `.sig` |
| macOS | `macos` → `.dmg` | `macos_updater` | `.app.tar.gz` + `.sig` |
| Linux deb | `linux_deb` → `.deb` | — | 不支持自动升级 |
| Linux AppImage | `linux_appimage` | `linux_appimage_updater` | `.AppImage` + `.sig` |

首版 Windows updater 使用 **MSI + .sig**（与现有 MSI 上传一致，减少构建变更）。若静默安装体验不佳，再切换 NSIS。

### 5.3 构建与上传（官网发版）

构建时在环境中注入 updater 签名密钥：

```bash
export TAURI_SIGNING_PRIVATE_KEY="..."
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="..."
npm run build:macos:signed   # 或各平台 build 命令
```

构建完成后，**必须**通过官网控制台上传（与现有发版流程相同）：

| 上传槽位 | 文件 |
|---------|------|
| Windows · MSI | 官网安装包 |
| Windows · Updater | updater 用 `.msi` + `.sig` 内容 |
| macOS · Universal | 官网 `.dmg` |
| macOS · Updater | `.app.tar.gz` + `.sig` |
| Linux · deb / AppImage | 官网安装包 |
| Linux · AppImage Updater | `.AppImage` + `.sig` |

上传完毕 → 点击「发布」→ 官网下载与自动升级 **同时上线**。

---

## 6. 服务端设计（pointer-official）

### 6.1 数据模型

在 `app_release_artifacts` 表增加字段：

```sql
ALTER TABLE app_release_artifacts ADD COLUMN signature TEXT NULL;
```

- `signature`：`.sig` 文件全文（Tauri manifest 要求内联，不能是 URL）
- 新增 `AppReleaseArtifactKind` enum 值：`windows_updater`、`macos_updater`、`linux_appimage_updater`

可选：同版本增加 `release_notes TEXT` 字段（或在 `app_release_versions` 新表存版本级 notes）。首版可在 admin 上传时填 notes，或从 CHANGELOG 粘贴。

### 6.2 更新 manifest API

```
GET /api/updates/latest
  ?target=darwin|windows|linux
  &arch=x86_64|aarch64
  &current_version=0.1.1
```

**行为：**

| 条件 | HTTP | 响应 |
|------|------|------|
| 无已发布版本 | 204 No Content | 空 |
| `current_version >= published_version` | 204 No Content | 空 |
| 有更新且该平台有 updater 产物 | 200 | Tauri JSON（见下） |
| 有更新但该平台无 updater 产物 | 204 | 客户端不提示（用户仍可从官网手动下载） |

**200 响应体（Tauri 标准）：**

`platforms.*.url` **必须**指向官网已有下载接口（与 `/download` 页同源）：

```json
{
  "version": "0.1.2",
  "notes": "修复若干问题，优化性能",
  "pub_date": "2026-07-12T04:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://pointer-api.readflowai.com/api/downloads/files/{uuid}",
      "signature": "<sig file content>"
    },
    "darwin-x86_64": { "url": "...", "signature": "..." },
    "windows-x86_64": { "url": "...", "signature": "..." },
    "linux-x86_64": { "url": "...", "signature": "..." }
  }
}
```

平台 key 映射：

| Tauri `{{target}}` + `{{arch}}` | DB kind |
|----------------------------------|---------|
| `darwin` + `aarch64` | `macos_updater`（Universal 单包同时填 aarch64 与 x86_64） |
| `darwin` + `x86_64` | 同上 |
| `windows` + `x86_64` | `windows_updater` |
| `linux` + `x86_64` | `linux_appimage_updater` |

macOS Universal Binary：同一 `.app.tar.gz` 同时写入 `darwin-aarch64` 与 `darwin-x86_64` 两个 key。

### 6.3 后台上传

`AppReleaseAdminPanel` 新增上传槽位：

| 槽位 | 接受文件 |
|------|---------|
| Windows Updater | `.msi` + 同行上传或自动配对 `.sig` |
| macOS Updater | `.app.tar.gz` + `.sig` |
| Linux AppImage Updater | `.AppImage` + `.sig` |

上传接口扩展：

- 方案 A（推荐）：一次上传 updater 包，同时 POST `.sig` 文本字段
- 方案 B：上传 `.sig` 为独立小文件，服务端读取内容写入 `signature` 列

发布逻辑不变：**同一次「发布」** 后，`GET /api/updates/latest` 与 `GET /api/downloads` 同步生效；撤销发布或版本退回草稿时，两端同时不可用。

### 6.4 存储与多实例

沿用 `APP_RELEASES_STORAGE_DIR` 共享存储要求（见 `pointer-official/docs/app-releases-deployment.md`）。

---

## 7. 客户端设计（pointer-app）

### 7.1 Rust 层

- 添加 `tauri-plugin-updater`（desktop only）
- `src-tauri/src/lib.rs` 注册插件
- 新增 `src-tauri/src/updater_commands.rs`：
  - `check_for_update()` → 返回 `{ available, version, notes, currentVersion }`
  - `download_and_install_update()` → 触发下载+安装，emit 进度事件
  - `relaunch_app()` → 调用 `app.restart()`

进度事件（Tauri emit）：

```typescript
{ event: 'updater://download-progress', payload: { downloaded: number, total: number | null } }
{ event: 'updater://status', payload: { phase: 'checking' | 'downloading' | 'ready' | 'error', message?: string } }
```

### 7.2 前端（Vue）

新增模块：

| 文件 | 职责 |
|------|------|
| `src/composables/useAppUpdater.ts` | 检查/下载/重启逻辑、定时器 |
| `src/components/updater/UpdateReadyBanner.vue` | 下载完成后的轻量提示条 |
| `src/components/settings/panels/AboutSettingsPanel.vue` | 版本号、「检查更新」按钮 |

**启用条件：**

- `isTauriRuntime()` 为 true
- `deployment_mode` 为 **platform**（非 standalone）
- `platform_endpoints::api_base()` 非空

Updater endpoint 与 OAuth 共用 `api_base()`，保证联调/生产域名一致。

### 7.3 用户体验流程

```
App 启动
  └─ 30s 后 → check_for_update()（静默）
       ├─ 无更新 → 结束
       └─ 有更新 → download_and_install_update()（后台）
            ├─ 下载中 → 可选：状态栏小图标/不打扰
            └─ 下载完成 → UpdateReadyBanner
                 ├─ 「立即重启」→ relaunch_app()
                 ├─ 「稍后」→  dismiss（下次启动再提示）
                 └─ 「跳过此版本」→ localStorage 记录 skippedVersion

每 6 小时 → 重复检查（若未 ready 且未 skip）

设置 → 关于 → 「检查更新」→ 同步 check，有更新则同上
```

**文案原则（界面规范）：** 简洁、面向用户，不出现「manifest / sig / updater artifact」等开发术语。

示例：

- 检查中：「正在检查更新…」
- 下载中：「正在下载新版本 {{version}}…」
- 就绪：「新版本 {{version}} 已就绪，重启后生效」
- 失败：「更新失败，请稍后重试或前往官网下载」（打开 `platform_endpoints::web_base()/download`）

### 7.4 错误处理

| 场景 | 处理 |
|------|------|
| 网络超时 | 静默失败，下次定时重试；手动检查时 toast 提示 |
| 签名校验失败 | 记录 warn 日志，toast「更新包校验失败」 |
| 磁盘空间不足 | toast 明确提示 |
| 用户拒绝重启 | 保留已下载包，下次启动 updater 插件可直接 relaunch |
| API 204 | 视为无更新 |

### 7.5 本地持久化

`localStorage`（或现有 settings 存储）：

```typescript
interface UpdaterPrefs {
  skippedVersion?: string   // 用户跳过的版本
  lastCheckAt?: number      // 上次检查时间戳
}
```

---

## 8. 发布流程（运维 — 与官网发版一致）

```
1. bump pointer-app 版本号（tauri.conf.json / package.json）
2. 本地或任意环境构建（含 updater 产物 + .sig）
3. 登录官网控制台 → 后台管理 → 客户端发布
4. 输入版本号，上传各平台：
   - 官网安装包（MSI / DMG / deb / AppImage）
   - 对应 Updater 包 + signature
5. （可选）填写 release notes
6. 点击「发布」—— 官网 /download 与客户端自动升级同时生效
7. 验收：
   - 官网 /download 页版本与下载链接正确
   - curl GET /api/downloads
   - curl GET /api/updates/latest?target=darwin&arch=aarch64&current_version=<旧版>
   - 旧版桌面客户端 → 后台下载 → 重启后版本正确
```

**注意：** 未在控制台发布的版本，客户端 **不会** 收到更新，即使本地存在构建产物。

---

## 9. 测试计划

| 用例 | 平台 |
|------|------|
| 无更新（204） | 全平台 |
| 有更新 → 后台下载 → 重启生效 | macOS, Windows, AppImage |
| 跳过此版本 | 全平台 |
| 手动「检查更新」 | 全平台 |
| 网络断开 | 全平台 |
| 签名被篡改 | 全平台 |
| deb 用户无 updater 产物 → 不弹窗 | Linux |
| 跨版本升级 0.1.1 → 0.1.2 | 全平台 |

macOS 测试必须使用 **Developer ID 签名 + 公证** 的包，否则 updater 安装会被 Gatekeeper 拦截。

---

## 10. 风险

| 风险 | 缓解 |
|------|------|
| updater 私钥丢失 | 1Password / CI Secret 双备份 |
| macOS 未签名 | 发布检查清单强制 signed build |
| Linux deb 用户期望自动升级 | 文档与 FAQ 说明，引导 AppImage |
| 大安装包下载占带宽 | 首版全量；后续可考虑 CDN |
| 更新中断用户任务 | 仅重启前确认，不强制立即重启 |

---

## 11. 二期扩展（预留）

- `min_supported_version`：登录或 check 时强制升级
- Release notes 富文本 / 多语言
- Beta channel（endpoint 或 query param）
- 更新统计（check / download / install 埋点，复用现有 `client_env.app_version` 上报链路）

---

## 12. 相关文档

- 现有发布部署：`pointer-official/docs/app-releases-deployment.md`
- 跨平台构建：`pointer-app/docs/contributing/cross-platform-build.md`
- macOS 签名：`pointer-app/signing/macos/README.md`
- Tauri updater 官方文档：https://v2.tauri.app/plugin/updater/
