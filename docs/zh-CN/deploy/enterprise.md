# 企业内网部署

[English](../../en/deploy/enterprise.md) | 简体中文

这一篇讲**一套装完**：控制面 + 客户端（或服务端）。

企业内网部署不是「装一个程序」，而是让三样东西互相认出来——控制面负责账号与发版，客户端负责日常使用，服务端（可选）给团队共用模型网关。缺哪一环都会出现「能登录但更新不了」「能发消息但看不到余额」这类半通状态。

只看单个产物怎么打，去 [打包与部署](README.md)。这一篇讲怎么把它们串起来。

---

## 1. 三个角色与拓扑

```
                    ┌──────────────────────┐
                    │  控制面 pointer-admin │  账号 / 权限 / 配额 / 发版
                    │  :8788               │
                    └───────┬──────────────┘
                            │ HTTPS
              ┌─────────────┴─────────────┐
              │                           │
    ┌─────────▼─────────┐       ┌─────────▼──────────┐
    │ 客户端（桌面 App） │       │ 服务端 pointer-server│  可选：团队共用模型网关
    │ managed 口味       │       │  :8787              │
    └───────────────────┘       └────────────────────┘
```

三者的关系：

| 角色 | 干什么 | 谁维护 |
|---|---|---|
| **控制面** | 登录、权限、配额、审计、发版、下载清单 | IT / 运维 |
| **客户端** | 员工日常使用，登录后从控制面取身份 | IT 打一次包发下去 |
| **服务端** | 团队共用一个模型网关（可选） | IT / 运维 |

**控制面是必需项，服务端是可选项。** 只有控制面也能用：员工各自填自己的模型 Key。

---

## 2. 第一步：部署控制面

<a id="control-plane"></a>

### 2.1 需要什么

- 一台 Linux 服务器（2 核 2 G 起步足够）
- 一个域名 + 有效证书（Cookie 鉴权与 OAuth 回调都要求）
- 数据库：默认 SQLite，生产建议 PostgreSQL

### 2.2 环境变量

| 变量 | 默认值 | 说明 |
|---|---|---|
| `POINTER_ADMIN_ADDR` | `127.0.0.1:8788` | 监听地址。前面有反代时保持回环，不要直接暴露 |
| `POINTER_ADMIN_DB_URL` | `sqlite:pointer-admin.db` | 数据库连接串 |
| `POINTER_ADMIN_STATIC_DIR` | 无 | 前端构建产物目录，必须指向 `admin/web/dist` |
| `POINTER_ADMIN_DATA_DIR` | 无 | 运行时数据（上传的安装包、模型目录缓存等） |
| `POINTER_ADMIN_PUBLIC_BASE` | 无 | 对外访问地址，如 `https://admin.example.com` |
| `POINTER_ADMIN_OAUTH_SECRET` | 无 | OAuth 客户端密钥，**必填**，见 §2.4 |
| `POINTER_ADMIN_SERVER_REDIRECT` | `http://127.0.0.1:8787/auth/callback` | 服务端回跳地址，只有部署服务端时才需要改 |
| `POINTER_ADMIN_TENANT` | 默认租户 | 多租户部署时用 |
| `POINTER_ADMIN_COOKIE_SECURE` | 见代码 | 走 HTTPS 时必须为真 |
| `POINTER_ADMIN_CAPTCHA` | 见代码 | 内网可关；公网建议开 |

会话有效期三个变量：`POINTER_ADMIN_SESSION_TTL`、`POINTER_ADMIN_ACCESS_TTL`、`POINTER_ADMIN_REFRESH_TTL`。

### 2.3 首启账号

**第一次启动必须给一个能登录的账号，否则服务起来了也没人进得去。** 启动日志会明确提示：

```
库里还没有任何账号。请设置 POINTER_ADMIN_BOOTSTRAP_USER 与
POINTER_ADMIN_BOOTSTRAP_PASSWORD 后重启，否则没人能登录。
```

```bash
POINTER_ADMIN_BOOTSTRAP_USER=admin \
POINTER_ADMIN_BOOTSTRAP_PASSWORD='换成强密码' \
./pointer-admin
```

首次启动会自动建表、写入默认租户，不需要手工跑 SQL。账号建成后可以把这两个变量去掉。

### 2.4 OAuth 客户端自动登记

设了 `POINTER_ADMIN_OAUTH_SECRET` 之后，控制面启动时会自动登记两个客户端：

| 客户端 | 类型 | 回跳地址 |
|---|---|---|
| 服务端 | 机密 | `POINTER_ADMIN_SERVER_REDIRECT` |
| 桌面端 | 公开（回环） | 无，走 `127.0.0.1` 回环 |

**不需要手工插库。** 这个 secret 要和客户端构建时用的一致，否则换票失败。

### 2.5 反代与 TLS

```nginx
location / {
    proxy_pass http://127.0.0.1:8788;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

`X-Forwarded-Proto` 必须传——控制面靠它判断是不是 HTTPS，决定 Cookie 要不要加 `Secure`。

### 2.6 静态资源

控制面自己不做前端构建，它只负责把 `POINTER_ADMIN_STATIC_DIR` 里的文件发出去。所以部署顺序是：

```bash
npm --prefix admin/web ci
npm --prefix admin/web run build        # 产物 admin/web/dist
cargo build --release -p pointer-admin

POINTER_ADMIN_STATIC_DIR=$PWD/admin/web/dist ./target/release/pointer-admin
```

漏了这一步，打开首页就是 404 或白屏。

---

<a id="client"></a>
## 3. 第二步：构建客户端

客户端从开源仓构建，加一个烘焙进二进制的控制面地址。

### 3.1 构建

```bash
POINTER_EDITION=managed \
POINTER_API_BASE=https://admin.example.com \
npm run tauri:build
```

`POINTER_EDITION=managed` 但缺控制面地址时，`crates/pointer-core/build.rs` **直接 panic**，构建失败。这是刻意的——避免产出一个「看起来是企业版、实际谁都不连」的半绑定包。

### 3.2 签名密钥对

自动更新需要一对签名密钥：

```bash
# 生成（私钥严格保管，不进仓库）
npm run tauri signer generate -- -w ~/.pointer/updater.key
```

私钥旁边会生成 `.pub`。**构建时如果私钥旁边有 `.pub`，构建脚本会嵌这把公钥**，不需要改 `src-tauri/tauri.conf.json`。

> 漏了 `.pub` 就会用仓库里那把公钥，你签的包验不过，**更新会静默失效**——没有报错，只是永远提示「已是最新」。

---

## 4. 第二步（可选）：部署服务端

<a id="server"></a>

团队要共用一个模型网关时才需要。产物与客户端不同：

```bash
npm run server:build
# target/release/pointer-server-bundle/pointer-server-{平台}-{架构}.zip
```

服务端的回跳地址要和控制面的 `POINTER_ADMIN_SERVER_REDIRECT` 对齐，否则 OAuth 换票失败。

不需要共用网关的话跳过这一节——员工各自在客户端里填模型 Key 即可。

---

## 5. 第三步：验证连通

装完之后按这四步确认，**每一步都要真看到结果**，不要凭「服务起来了」判断。

| # | 验证什么 | 怎么做 | 期望 |
|---|---|---|---|
| 1 | 控制面活着 | 浏览器打开 `https://admin.example.com/` | 出现登录页，不是 404/白屏 |
| 2 | 能登录 | 用 §2.3 的账号登录 | 进入控制台，导航完整 |
| 3 | 客户端认控制面 | 打开构建好的客户端 → 登录 | 登录跳转到 `admin.example.com`，不是别的域名 |
| 4 | 链接指向正确 | 客户端里点「余额」 | 跳到 `admin.example.com` 下的账单页 |

第 3、4 步最关键——它们证明客户端里烘焙的地址确实生效了。如果跳到官方域名，说明构建时 `POINTER_API_BASE` 没生效。

---

## 6. 自动更新

控制面提供 `GET /api/updates/latest?target=…&arch=…&current_version=…`，客户端从这个地址取更新清单。

两个前提：

1. **安装包要有签名**。清单会跳过未签名的产物，没签名的包等于没上传。
2. **公钥要配对**。客户端构建时嵌的公钥，必须和签这个包的私钥是一对。

上传入口在控制台的发版页，需要 `release:write` 权限。

---

## 7. 模型目录

**自建控制面不提供平台模型目录。** 员工登录后看到的是空目录，需要自己填模型服务商的 API Key。

这是设计使然：开源版不内置任何厂商凭据。如果企业想统一采购模型额度，那是私有化扩展该做的事，不在开源仓范围内。

---

## 8. 上线检查清单

- [ ] 域名与证书就绪，`X-Forwarded-Proto` 已透传
- [ ] `POINTER_ADMIN_OAUTH_SECRET` 已设，且与客户端构建时一致
- [ ] `POINTER_ADMIN_BOOTSTRAP_USER` / `PASSWORD` 已设，账号可登录
- [ ] `POINTER_ADMIN_STATIC_DIR` 指向前端产物，首页不是白屏
- [ ] `POINTER_ADMIN_DATA_DIR` 可写（上传安装包要用）
- [ ] 数据库已备份策略（SQLite 单文件 / PG 常规备份）
- [ ] 客户端构建日志里 `POINTER_EDITION=managed` 且域名烘焙成功
- [ ] 签名私钥旁有 `.pub`，构建嵌的是自己的公钥
- [ ] 至少上传过一个**带签名**的安装包
- [ ] 客户端登录跳转到自己的控制面域名
- [ ] 客户端「余额」链接指向自己的控制面
- [ ] 关掉不需要的功能（内网可关验证码）

---

## 9. 排错

| 现象 | 先查 |
|---|---|
| 首页白屏 / 404 | `POINTER_ADMIN_STATIC_DIR` 指对了没，前端构建跑过没 |
| 服务起来了但登不进去 | 首启账号设了吗，日志有没有那句提示 |
| 登录后立刻掉线 | `POINTER_ADMIN_COOKIE_SECURE` 与 HTTPS 是否匹配；反代有没有传 `X-Forwarded-Proto` |
| 客户端登录跳到官方域名 | 构建时 `POINTER_API_BASE` 没生效，检查构建日志 |
| 一直提示「已是最新」 | 签名公钥不配对，或安装包没签名 |
| 客户端登录报换票失败 | 控制面的 `OAUTH_SECRET` 与客户端不一致 |

---

## 相关文档

- [打包与部署](README.md) —— 口味与运行形态的总览
- [构建产物与口味](editions.md) —— 单个产物怎么打
- [平台打包](platforms.md) —— 三个平台的差异
- [桌面端 OAuth 对接](../developer/desktop-oauth-web-integration.md) —— 换票细节
