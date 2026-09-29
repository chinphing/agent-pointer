# 控制面绑定（本地 / 域名）

同一份源码，两种运行形态。差别只在**有没有绑定控制面域名**，不是两套产品逻辑。

| 形态 | 含义 |
| --- | --- |
| **未绑定域名** | 纯本地 Agent：模型、Key、通道等只依赖本机设置；默认不连外部账户云 |
| **绑定域名** | 走同一套控制面逻辑（登录、目录、用量等）。官网发包装 `readflowai.com`；企业装自己的内网域名 |

打包口味仍用 `POINTER_EDITION` / `VITE_POINTER_EDITION`：

| 值 | 效果 |
| --- | --- |
| 未设置（**personal 构建**） | 不预填官方域名（未绑定 ⇒ standalone）；关 Tauri 更新产物（`tauri.personal.conf.json`） |
| `official` | 要求控制面域名（**构建期注入，缺失则构建失败**）；官方更新端点与 standalone License 策略 |

源码不硬编码任何官方域名。控制面域名在**构建期**注入：`POINTER_API_BASE`、`POINTER_WEB_BASE`、`COMPUTER_ANNOTATE_API_BASE`（由 `crates/pointer-core/build.rs` 读入并烧进二进制）；前端对应 `VITE_POINTER_EDITION`、`VITE_POINTER_WEB_BASE`、`VITE_POINTER_DOWNLOAD_URL`。运行时仍可用同名 `POINTER_*` 环境变量覆盖，`POINTER_USAGE_REPORT_ENABLED` 亦同。

## 本机默认：`pointer.local.env`（不提交）

每位开发者在仓库根目录放一份**已被 gitignore** 的本地文件，日常 `tauri:dev` / `tauri:build` / Vite 会自动加载（**已存在的 OS / CI 环境变量优先**）。

```bash
cp pointer.local.env.example pointer.local.env
# 按需编辑 pointer.local.env
```

示例见仓库根目录 [`pointer.local.env.example`](../../pointer.local.env.example)。

| 角色 | 建议 |
| --- | --- |
| 开源 / 个人 | 不建文件（不设域名即未绑定 ⇒ standalone） |
| 官网维护者 | 在 `pointer.local.env` 写 `official` + 官网 API/Web/SOM 域名 |
| 企业内部 | 同样写 `official`（或等价绑定），域名改成内网控制面 |

加载实现：`scripts/lib/load-pointer-local-env.mjs`（由 `tauri-dev.mjs` / `tauri-build.mjs` / `vite.config.ts` 调用）。`POINTER_*` 与对应 `VITE_*` 会互相补齐空白项。

## 怎么打

未设置环境变量且无本地文件时，`npm run tauri:build` **默认本地包（未绑定 ⇒ standalone）**：

```bash
npm run tauri:build
```

绑定控制面（本机已写好 `pointer.local.env`，或显式导出变量）：

```bash
# 依赖 pointer.local.env 里的 official + 域名
npm run tauri:build

# 或一次性覆盖（CI / 临时）
POINTER_EDITION=official VITE_POINTER_EDITION=official \
  POINTER_API_BASE=https://pointer-api.readflowai.com \
  POINTER_WEB_BASE=https://pointer.readflowai.com \
  VITE_POINTER_WEB_BASE=https://pointer.readflowai.com \
  npm run tauri:build
```

官方发版由 [`.github/workflows/release.yml`](../../.github/workflows/release.yml) 注入 `POINTER_EDITION=official`、控制面域名（repository variables，见该 workflow 的 env 段）与签名 secret，不依赖开发者本机的 `pointer.local.env`。

## 日常开发

```bash
npm run tauri:dev   # 自动读 pointer.local.env；无文件则不绑定控制面（standalone）
```

不要把生产域名、签名口令写进公开 `main`。需要联调控制面时只改本机 `pointer.local.env`。

## 维护流程

- 通用功能：公开仓库 PR → 本地包与绑定控制面的包共用
- 只对付费云能力：改私有云 / 计费 / 更新服务
- 换域名、更新公钥、证书：只改官方 CI 或维护者本机 `pointer.local.env`，勿提交
- 外部 PR 默认按未绑定理解；合入前确认没有写死某一家控制面域名

## 发布前检查

- 签名与密钥只放在 CI 的 secret / environment 里，不写进仓库、不写进本机配置文件
- 在 GitHub 打开 Private vulnerability reporting
