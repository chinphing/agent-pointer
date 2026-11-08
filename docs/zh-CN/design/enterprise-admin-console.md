# 企业控制面（管理端）设计

状态：设计稿，未实现。M0 待开工。
仓库：agent-pointer。产物：`pointer-server` + 同一套 Web 界面。

本文是企业自建控制面的设计方案，不是用户教程。
「管理端」= 企业管理员使用的后台；「员工端」= 员工使用的 Pointer 界面。
管理端与员工端**同一个进程、同一份前端**，靠权限与路径区分。

## 1. 背景与决策记录

企业需要一套和官网同角色、但部署在自己机房的控制面：员工登录后**不用自己填模型 Key**，
自动拿到企业统一的模型与技能；管理员在后台管这些。

| # | 决策 | 结论 | 影响 |
| --- | --- | --- | --- |
| 1 | 代码基座 | **只参考**官网（`pointer-official`）的交互与字段，不继承其代码与表 | 全部在 `server` + `pointer-core` 内新建 |
| 2 | 多租户 | 设计上保留，功能上先单租户 | 所有业务表带 `tenant_id`，单租户期恒为 `default` |
| 3 | 登录方式 | 本地账号（管理员建号 + CSV 导入，不自注册） | 需要多用户表与每人独立密码哈希 |
| 4 | 企业 Key 防外带 | **接受**明文下发风险，靠配额 + 审计 + 协议兜底 | 不做网关 token；`key_scope` 字段预留 |

决策 4 是**已知且被接受的敞口**，不是缺陷：员工可从浏览器或客户端内存中取走企业 Key。
缓解只有三件软约束，M0 必须全部具备：每人月度配额与超限拦截、Key 的 reveal 与变更全部进审计、
资源表预留 `key_scope` 以便日后切"每人独立 Key"或"网关 token"。

## 2. 现状与缺口

server 现在是「单管理员 + 无用户体系」的壳，四件基础设施缺失：

| 缺口 | 现状 | 证据 |
| --- | --- | --- |
| 没有用户表 | 账密只有一个来源：配置里的 `username` + `password_hmac` + `hmac_secret` | `crates/pointer-core/src/local_auth.rs:3-53` |
| 会话在内存 | `WebSessionStore` 是 `RwLock<HashMap<..>>`，重启即全员掉线 | `server/src/web_session.rs:29-32` |
| 没有管理员判定 | `/api/admin`、`is_admin`、`require_admin` 在 server 侧 0 命中；`isPlatformAdmin` 在 standalone 恒 true | `crates/pointer-core/src/chat_service/app_state.rs:1236-1251` |
| 平台层 Key 无处可配 | `pointer-server.toml` 的 `[llm]` 段已废弃并被忽略 | `crates/pointer-core/src/server_config.rs:1165-1181` |

登录后塞进会话的凭证是空结构（`server/src/local_auth.rs:214`），
所以「统一 Key」在 server 侧不是没接上，而是**连配置位都已删除**。

## 3. 架构：admin 是控制面，server 与客户端都是它的 OAuth 客户端

```text
管理员浏览器 ──→ admin (:8788)   ← 管理后台 + 登录页 + OAuth + partner API
                    ▲   ▲
员工浏览器 ──→ server (:8787) ┘   │
                    ▲             │
桌面客户端 ─────────┴─────────────┘
```

| 角色 | 职责 | 不再负责 |
| --- | --- | --- |
| **admin**（控制面，独立进程） | 账号、Key、资源、配额、审计；对外提供 OAuth 与 partner API | 对话、工作区、会话存储 |
| **server**（资源服务器） | Web 界面、会话、对话执行 | 用户表、登录页、密码校验 |
| **桌面客户端** | 本机 Agent | 与 server 同构，同一个 client 协议 |

**接 admin 对 server 是配置变更，不是代码变更**——OAuth 客户端与 partner 拉取逻辑已经写好：
`server/src/main.rs` 的 `/api/auth/login/start`、`/api/auth/oauth/callback`，
`crates/pointer-core/src/platform_auth.rs` 的 token 交换与凭证刷新。

`{app_data}/admin.db` 属于 admin 进程，与 server 的 `conversations.db` 分属两个进程、两处存储。
不塞进 `conversations.db`：那边已是 schema v28，改动风险大。

### 3.1 admin 必须实现的接口契约

| 方法 | 路径 | 消费方 | 客户端现有实现 |
| --- | --- | --- | --- |
| GET | `{web}/oauth/authorize`（PKCE） | 浏览器，server 与客户端共用 | `platform_auth.rs:645` |
| POST | `{api}/auth/oauth/exchange-code` | server | `cloud_agent_auth.rs:79` |
| POST | `{api}/auth/app/token` | 桌面客户端 | `platform_auth.rs:825` |
| POST | `{api}/auth/app/logout` | 桌面客户端 | `platform_auth.rs:1310` |
| GET | `{api}/auth/partner/llm-credentials` | server / 客户端 | `platform_auth.rs:1042` |
| GET | `{api}/auth/partner/balance` | server / 客户端 | `platform_auth.rs:1173` |
| POST | `{api}/auth/partner/token-usage` | server / 客户端 | `platform_auth.rs:946` |

请求与响应的字段形状**以 `platform_auth.rs` 的结构体为准——客户端代码就是规范**，
不另立一套 JSON，否则要同时改客户端与 server。

### 3.2 新增一张表：`oauth_clients`

| 字段 | 说明 |
| --- | --- |
| `client_id` / `client_secret_hash` | `pointer-server`、`pointer-desktop` 两类 |
| `redirect_uris` | 逗号分隔白名单，换码时严格比对 |
| `kind` | `server` / `desktop` |

客户端现有常量是 `DEFAULT_OAUTH_CLIENT_ID = "pointer-desktop"`（`platform_endpoints.rs:18`），
server 侧另有 `POINTER_OAUTH_CLIENT_SECRET`；admin 必须按这两者登记，不能改名。

## 4. RBAC

### 4.1 权限串

`<domain>:<action>`，domain 与资源的 `kind` 一一对应：

| domain | 覆盖 |
| --- | --- |
| `user` | 成员账号 |
| `role` | 角色与授权 |
| `provider` | 模型服务商与密钥（含 `reveal`） |
| `model` | 模型目录与档位 |
| `skill` | 企业技能库 |
| `plugin` | 企业插件库 |
| `mcp` | 全局 MCP 配置 |
| `usage` | 用量与配额 |
| `audit` | 审计流水 |
| `system` | 版本、许可证、存储 |

action 取值：`read` / `write` / `publish` / `delete` / `reveal`。
权限清单定义在代码常量里，DB 只存「角色 → 权限」的授权关系，避免权限表迁移。

### 4.2 预置角色

| key | 名称 | 默认权限 | 可否删除 |
| --- | --- | --- | --- |
| `super_admin` | 超级管理员 | 隐式全部（含 `role:write`） | 否 |
| `admin` | 管理员 | 除 `role:write`、`provider:reveal` 外全部 | 否 |
| `viewer` | 运营（只读） | 所有 `:read` | 否 |
| `member` | 成员 | 无管理端权限 | 否 |
| 自定义 | — | 勾选权限 | 是 |

`member` 没有管理端任何权限，因此天然进不了 `/admin`——**不需要单独的 is_admin 布尔位**。

### 4.3 两层授权，不要混

| 层 | 回答的问题 | 表 |
| --- | --- | --- |
| 操作权限（RBAC） | 管理员能操作管理端的哪些功能 | `roles` / `role_permissions` / `user_roles` |
| 资源可见范围 | 员工能看到并使用哪些资源 | `resource_grants` |

员工被授予某个技能，不等于他能进管理端；管理员能改技能，也不等于该技能对所有员工可见。

## 5. 统一资源模型

技能与模型、插件、MCP 走**同一套存储、同一套 CRUD、同一套授权与审计**，
差异只体现在 `kind` 与 `payload`。

| kind | payload | 二进制 | 特有动作 |
| --- | --- | --- | --- |
| `provider` | base_url、模型列表 | — | `reveal`（取明文 Key，需 `provider:reveal`） |
| `model` | 档位、排序、可见性 | — | — |
| `skill` | 名称、描述、tags、allowed-tools | zip | `publish`（新版本） |
| `plugin` | manifest、来源信任级别 | zip | `publish` |
| `mcp` | 服务定义 | — | — |

新增一类资源 = 加一个 `kind` 常量 + 一组权限 + 一个 payload 结构，**不需要新表、不需要新授权逻辑**。

### 5.1 单资源授权（grants）

「可见范围」不落列，**全部由 `resource_grants` 派生**。三种 principal：

| `principal_type` | `principal_id` | 含义 |
| --- | --- | --- |
| `all` | 空 | 全员可见 |
| `role` | 角色 id | 该角色的所有成员可见 |
| `user` | 用户 id | 仅该成员可见（**单人授权**） |

判定规则（只有并集，没有 deny）：

```text
有效可见 = 存在 all 行
         ∨ 用户的任一角色出现在 role 行
         ∨ 用户的 user_id 出现在 user 行
```

| 行数 | 语义 | 界面 |
| --- | --- | --- |
| 0 行 | 未授权（草稿态，仅管理员可见） | 灰字「未授权」 |
| 1 行 `all` | 全员 | 单选「全员」 |
| 任意 `role` / `user` 行 | 指定 | 单选「指定」，chips 混排角色与个人 |

三条硬规则：

1. **`all` 与明细行互斥**：选「全员」时清空其他行，避免出现「全员 + 财务组」这类冗余叠加；
2. **不做 deny（排除）**：全员技能不能单独把某人排除。需要排除就把该资源改成「指定」再逐个授；
   排除会让「为什么张三看不到」变成两处状态叠加，排查成本高于收益；
3. **授权挂在资源上，不挂版本**：发布 v2.3 不改变授权对象，避免每次发版重配。

与 RBAC 的区别（最容易混的一点）：`skill:write` 是「能不能改这个技能的授权」，
不是「能不能用这个技能」。前者属操作权限，后者属 grants。

接口契约：`PUT /api/admin/resources/:id/grants` 为**整体替换**（幂等），
前端做增删后提交全集；每次替换把 before/after 写进 `audit_log`。

员工端不判断权限：`GET /partner/resources` 由服务端按 `user_id` join 过滤后返回，
未授权资源**连名称都不出现**，客户端拿到的就是自己可见的全集。

## 6. 表结构（`{app_data}/admin.db`）

| 表 | 关键字段 | 阶段 |
| --- | --- | --- |
| `tenants` | id / name / status | P0（只一行 `default`） |
| `users` | id / **tenant_id** / username / display_name / password_hash / status / last_login_at | P0 |
| `roles` | id / **tenant_id** / key / name / builtin | P0 |
| `role_permissions` | role_id / permission | P0 |
| `user_roles` | user_id / role_id | P0 |
| `sessions` | id / user_id / kind / expires_at | P0 |
| `resources` | id / **tenant_id** / kind / slug / name / description / **parent_id** / enabled / payload_json / **secret_enc** / current_version_id | P0 |
| `resource_versions` | id / resource_id / version / dir_hash / blob_path / size_bytes / uploaded_by | P0 |
| `resource_grants` | id / resource_id / principal_type(`all`\|`role`\|`user`) / principal_id（`all` 时为空） | P0 |
| `quotas` | user_id / month / token_limit / token_used / over_action(`block`\|`warn`) | P0 |
| `usage_daily` | **tenant_id** / user_id / domain / resource_slug / day / prompt_tokens / completion_tokens | P0 |
| `audit_log` | **tenant_id** / actor_user_id / action / resource_id / before_json / after_json / at | P0 |

约定：

- 唯一索引一律含租户：`UNIQUE(tenant_id, username)`、`UNIQUE(tenant_id, kind, slug)`；
- `tenant_id` 从会话上下文取，代码里不写死字符串；
- `secret_enc` 复用现有 AES-256-GCM `enc:v1:`（`crates/pointer-core/src/storage.rs:695-730`），不新造加密；
- 密码用 argon2id；现有单条无盐 HMAC 不可用于多用户。

## 7. 统一 Key 的下发链路

`PlatformLoginCredentials` 是唯一接缝。把它的来源从空结构换成「查 admin.db」，
**前端与 `pointer-core` 一行都不用改**。

```text
admin.db（provider + model 资源）
  → admin 的 GET /auth/partner/llm-credentials
  → server / 客户端的 platform_auth（crates/pointer-core/src/platform_auth.rs:1042）
  → PlatformLoginCredentials { platform_providers, provider_api_keys, model_catalog, tier_defaults }
  → WebSessionEntry.creds → platform_config（chat_service/app_state.rs:1399-1418）
  → 员工对话：模型列表与 Key 全部自动就位
```

**客户端与 server 一行都不用改**：这条链路本来就是为远程控制面写的，admin 只是把「官网」换成了内网地址。

Key 轮换靠 `model_catalog_hash` 比对（`crates/pointer-core/src/chat_service/session_inner.rs:171-178`）。
**前提是 hash 必须覆盖密钥指纹**，否则只换 Key、不换模型目录时 hash 不变，员工端不会重新拉取，
旧 Key 会一直用到重新登录：

```text
model_catalog_hash = sha256( model_catalog ‖ tier_defaults ‖ 每个 provider 的 key_fingerprint )
key_fingerprint    = sha256(api_key) 的前 8 位十六进制
```

指纹只进 hash，不进任何下发给浏览器的响应。

### 7.1 密钥存储

| 项 | 方案 |
| --- | --- |
| 落库 | `resources.secret_enc`（`kind=provider`），AES-256-GCM |
| 加密材料 | 同算法独立实现（`admin/src/crypto.rs`）：HKDF(pepper, machine_uid) + 用途标签 `admin-provider-keys-v1`；**admin 不依赖 pointer-core** |
| 明文边界 | 只在服务端内存中出现；浏览器与员工端永远拿不到 |
| 运维后果 | 加密材料绑定本机——**换机、迁移、从备份恢复到新机器后 Key 需重新录入**；不支持多副本共享同一份 admin.db |
| 取明文 | `POST /api/admin/resources/:id/reveal`，需 `provider:reveal`，二次确认 + 写审计 |

若要支持可迁移（备份到别的机器仍能解），需要引入 `POINTER_SERVER_SECRET_KEY` 环境变量作为主密钥，
代价是密钥进了部署环境，运维必须保证它不入镜像、不进版本库。默认不做，等有迁移需求再评估。

### 7.2 档位映射（tierDefaults）

客户端把「档位」当作用户可选的性能档，三档固定：`fast` / `standard` / `expert`。
管理端配的是**「场景 × 档位 → 模型」的映射**，不是「员工用哪一档」——选哪一档仍由员工决定。

下发的 JSON 形状（`crates/pointer-core/src/models/settings.rs:3214` 的读取端）：

```json
{
  "agentModeLlm": {
    "general": {
      "fast":     {"providerId":"qwen","model":"qwen3.5-flash","thinkingIntensity":"off"},
      "standard": {"providerId":"qwen","model":"qwen3.5-plus","thinkingIntensity":"medium"},
      "expert":   {"providerId":"qwen","model":"qwen3.5-max","thinkingIntensity":"high"}
    },
    "coder":    { "...": {} },
    "computer": { "...": {} },
    "web_search": { "...": {} }
  },
  "mediaModeLlm": {
    "image": { "fast": {} }, "audio": { "fast": {} }, "video": { "fast": {} }
  },
  "computerPipelineLlm": {
    "locate": {}, "locateVerify": {}, "verify": {},
    "byTier": { "fast": { "locate": {}, "locateVerify": {}, "verify": {} } }
  },
  "agentDefaultModels": { "general": {} }
}
```

**合并语义是「补齐」，不是「强制」**：客户端只填缺失项，用户已显式配置的档位保持不变
（`apply_platform_tier_defaults` 里的 `contains_key` 与 `trim().is_empty()` 判断）。
所以管理员配的值 = 默认值；员工自己改过该档，管理员的配置就不生效。

要「强制统一档位」需要客户端新增锁定字段并在设置界面置灰档位选择器——**这不是纯管理端功能**，
M0 不做。M0 只做下发默认值。

**电脑操控的定位链**单独配（`computerPipelineLlm`）：`locate` / `locateVerify` / `verify`
三个槽位各自是 `{providerId, model, thinkingIntensity?}`；再叠一层
`byTier[fast|standard|expert]` 覆盖，留空即继承全局槽位。
管理端界面按「槽位 × 全局默认 / 三档」的矩阵配；`decision` / `position` 两个老槽位
客户端仍兼容，但不在界面露出。

四条约束：

1. **引用必须在可见目录内**：保存时校验 `providerId/model` 存在于 `model_catalog`，否则员工端拿到的档位指向一个看不见的模型；
2. **条目形状是 `{providerId, model, thinkingIntensity?}`**：`thinkingIntensity` 可选，
   取值 `off` / `low` / `medium` / `high` / `max`（`crates/pointer-core/src/thinking_strategy.rs:15-43`）；
3. **`maxTokens` / `enableThinking` / `thinkingBudget` 不在档位配**：它们在「模型」资源里配一次，
   档位只覆盖思考强度，避免 21 个格子各填一遍；
4. **`web_search` 有隐式回退**：目录里有 DeepSeek 时，未配置的档位自动用 DeepSeek 的第一个模型
   （`deepseek_web_search_tier`）。管理端要么显式配置，要么在界面上说明这条规则。

**思考强度的优先级**（`tier_cfg_from_json`，`models/settings.rs:3455-3458`）：

```text
档位自带 thinkingIntensity  >  模型级 model_configs  >  服务商级默认  >  客户端内置默认
```

档位写了强度就以档位为准——这是「平台下发档位口径」。但仍然是补齐语义：
**员工自己改过某档的强度，管理端的值不覆盖他**。

桌面自动化的校验链（`computerPipelineLlm.byTier[mode].locate / locateVerify / verify`）
同样支持 `{providerId, model, thinkingIntensity}`，但只在槽位为空时填。

存储：复用统一资源表，`kind=tier_profile`、`slug=default`，**M0 只有一条**；
M2 需要「不同部门不同档位」时，改成多条 + `resource_grants`，表结构不变。
档位改动同样参与 `model_catalog_hash`，员工下一轮对话生效。

技能下发复用同一模式（M1）：`resources` 的 `skill` 行 + `dir_hash` → 员工端拉取 zip →
落到 `{data_dir}/org-skills/` → `SkillProvenanceKind` 增加 `Org` 变体
（现有只有 `System` / `User` / `External`，见 `crates/pointer-core/src/skills/provenance.rs:17-31`）。

## 8. 接口

### 8.1 管理端（`/api/admin/*`，需对应权限）

| 方法 | 路径 | 权限 |
| --- | --- | --- |
| POST | `/api/admin/auth/login` | 公开（账密 + 验证码） |
| POST | `/api/admin/auth/logout` | 登录态 |
| GET | `/api/admin/me` | 登录态（返回角色与权限集） |
| GET/POST/PATCH/DELETE | `/api/admin/users` | `user:read` / `user:write` |
| POST | `/api/admin/users/:id/reset-password` | `user:write` |
| PUT | `/api/admin/users/:id/roles` | `role:write` |
| GET/POST/PATCH/DELETE | `/api/admin/roles` | `role:read` / `role:write` |
| PUT | `/api/admin/roles/:id/permissions` | `role:write` |
| GET | `/api/admin/permissions` | `role:read`（权限清单） |
| GET/POST/PATCH/DELETE | `/api/admin/resources?kind=` | `<domain>:read` / `<domain>:write` |
| POST | `/api/admin/resources/:id/versions` | `<domain>:publish` |
| POST | `/api/admin/resources/:id/enable`、`/disable` | `<domain>:write` |
| PUT | `/api/admin/resources/:id/grants` | `<domain>:write` |
| POST | `/api/admin/resources/:id/reveal` | `provider:reveal` |
| GET | `/api/admin/usage?from&to&group_by=user\|model\|day` | `usage:read` |
| GET/PUT | `/api/admin/quotas` | `usage:read` / `usage:write` |
| GET | `/api/admin/audit` | `audit:read` |
| GET | `/api/admin/overview` | `system:read` |

### 8.2 server 侧（有 admin 时几乎不改代码）

| 项 | 改动 |
| --- | --- |
| 部署模式 | `deployment_mode=platform`，`POINTER_API_BASE` / `POINTER_WEB_BASE` 指向 admin |
| 登录 | 现有链路：`/api/auth/login/start` → admin 的 `/oauth/authorize` → `/api/auth/oauth/callback` |
| 凭证 | `platform_auth` 拉 `/auth/partner/llm-credentials`，塞进 `WebSessionEntry.creds` |
| 门禁与上报 | `/auth/partner/balance`、`/auth/partner/token-usage` 已有；`is_standalone()` 为 false 时自动生效 |
| `local_auth` | 保留，但只在**没有 admin** 的纯离线部署使用 |

业务路由（`/api/skills`、`/api/plugins`、`/api/conversations` 等）M0 完全不动。
员工登录页由 admin 提供，server 不再有自己的登录界面。

## 9. 前端：管理端是独立工程

管理端有自己的 Vite 工程 `admin/web/`，**不复用客户端 `src/`**。

理由：客户端那套是聊天向（会话、消息、工作台、markdown 渲染），管理端是表格与表单。
混在一起会让桌面端打包管理端代码、共享 i18n 与依赖，改管理端还要重跑整个客户端构建。

| 项 | 客户端 / server | 管理端 |
| --- | --- | --- |
| 工程 | `src/`（根 `package.json`） | `admin/web/`（独立 `package.json`） |
| 产物 | `dist/`，server 与桌面共用 | `admin/web/dist`，由 admin 进程 serve |
| 语言 | zh + en，`localeParity.test.ts` 强制同键 | 只中文，不接 i18n |
| 主题 | `src/styles/globals.css` | 取值与客户端 token 对齐（zinc 灰阶 + `#0071EB`），见 `admin-mockups/` |
| 路由 | 无 router，两态 | vue-router hash 路由（刷新任意页不 404） |
| 页面 | 聊天 / 设置 | 概览 / 成员 / 角色 / 资源 / 用量 / 审计（6 页，档位映射是资源页的一个页签） |

界面线框图在 [`admin-mockups/`](admin-mockups/)（9 张，含登录、概览、资源、角色权限、
详情抽屉、授权浮层、新建模型服务、档位映射）。**改界面先看图，别自己发挥。**

「复制 CSS 变量」是唯一刻意的重复（约 30 行），换来客户端与 server 前端**零改动**。

登录入口有两个，各自独立：员工与桌面客户端走服务端渲染的 `/oauth/authorize`
（自包含页面，不依赖前端路由）；管理员走 SPA 的 `#/login`。

「资源」页是统一列表，用 `kind` 筛选出技能、模型、插件——这是「技能与其它功能同样管理」在界面上的直接体现。

## 10. 分期

| 阶段 | 内容 | 出口标准 | 粗估 |
| --- | --- | --- | --- |
| **M0** | 账号体系（表 / argon2 / sessions）+ RBAC + 资源表 + provider 与 model 资源 + **控制面 API 契约（OAuth + partner 七端点）** + 独立前端工程 + 用量 + 审计 | server 与桌面客户端都能通过 admin 登录，且**一个 Key 都不填**能对话；`viewer` 角色改不动任何东西；改 Key 后下一轮生效 | 18–24 人日 |
| **M1** | `skill` 资源 + zip 版本 + 下发 + 员工端 `org-skills` 与 `Org` 来源 | 管理员发布 v2 → 员工端内容更新；员工本地改动不被覆盖 | 10–14 人日 |
| **M2** | `plugin` 资源（含来源信任级别）+ 配额拦截 + 按角色下发不同档位 | 未授权成员看不到插件；超配额被拦 | 8–12 人日 |

M0 之所以多了 4–6 人日：**OAuth + partner 七端点必须一起做**，否则 server 连不上 admin。
好处是一次把 server 与桌面客户端都打通（两者同一个协议），原计划的「M2 桌面端绑定」消失了。
仍然不做：员工业务会话持久化（沿用内存，重启掉线可接受）。

## 11. 明确不做

- 不继承 `pointer-official` 的代码、表与店铺/计费/云主机逻辑；
- 不做员工自注册、部门树、审批流、组织架构同步；
- 不做租户切换界面与租户管理页（表与字段保留）；
- 不做细粒度数据权限（行级、字段级）；
- 不做网关 token 与每人独立 Key（`key_scope` 字段预留，功能后置）。

## 12. 风险

| 风险 | 说明 | 处置 |
| --- | --- | --- |
| 企业 Key 外带 | 决策 4 已接受 | 配额 + 审计 + 协议；字段预留 |
| 配置迁移 | 老部署只有 `[auth.local]` 单账号 | 首启把配置账号写成 `super_admin`，其余账号进表 |
| 会话重启掉线 | 员工业务会话仍为内存态 | M0 只把**管理端**会话落表，员工侧维持现状 |
| 资源表多态 | 单表 `kind` 分型，无外键约束 | 用 CHECK 约束 + 代码层 `kind` 枚举收敛 |
| 技能包安全 | zip 解压与路径穿越 | 复用现有 `skills/external.rs:12-13` 的体积上限与路径校验 |

## 13. 代码落点

```text
agent-pointer/
├── Cargo.toml                   改：members 加 "admin"
├── admin/                       新：独立控制面进程 + 独立前端工程
│   ├── Cargo.toml
│   ├── src/                     Rust（平铺，未做 store/ 子目录：模块名已足够区分）
│   │   ├── main.rs              读配置 → open db → 种子 → 起 axum :8788
│   │   ├── config.rs            POINTER_ADMIN_*
│   │   ├── state.rs             AppState（store + config；进程内无跨请求状态）
│   │   ├── db.rs / schema.rs    连接、迁移（v1→v3）、CREATE TABLE
│   │   ├── password.rs          argon2id
│   │   ├── crypto.rs            AES-256-GCM + HKDF(machine_uid)
│   │   ├── users / sessions / rbac / resources / grants
│   │   ├── quota → usage.rs     用量与配额
│   │   ├── audit.rs             审计流水
│   │   ├── credentials.rs       组装 PlatformLoginCredentials
│   │   ├── http/                管理后台 /api/admin/*（cookie + RBAC）
│   │   │   auth / users / roles / resources / insight / error
│   │   ├── oauth/               控制面契约（server 与客户端消费）
│   │   │   authorize / code / clients / token / partner
│   │   └── （静态资源：main.rs 里挂 ServeDir + 占位页，未单列 static_files.rs）
│   ├── scripts/smoke.sh         端到端冒烟：登录→建资源→OAuth→取凭证→报用量
│   ├── web/                     独立 Vite 工程（Vue 3 + hash 路由），产物被进程托管
│   │   ├── package.json  vite.config.ts  index.html  README.md
│   │   └── src/                 概览/成员/角色/资源/用量/审计 + api/store/router
│   └── tests/                   集成测试：起 router 打真实请求
├── server/src/main.rs           改 1 处：spa_fallback 把 /admin 302 到 admin
├── crates/pointer-contract/     新：三端共用的契约类型 + 哈希（无内部依赖）
│   └── src/{credentials,provider,session,hash}.rs
├── crates/pointer-core/
│   ├── src/platform_auth.rs     改：契约类型改为 pub use pointer_contract
│   └── src/skills/provenance.rs 改：M1 加 SkillProvenanceKind::Org
└── src/                         不动（客户端与 server 前端零改动）
```

依赖方向（**admin 与 server 之间没有 crate 依赖**）：

```text
admin ─────────────→ pointer-contract
pointer-core ──────→ pointer-contract
server ────────────→ pointer-core, pointer-channels
src-tauri ─────────→ pointer-core, pointer-channels
```

admin 与 server 只通过 HTTP 说话；指针不共享，桌面包体不受影响。

**客户端接入不需要新配置。** 桌面端与 server 早就只认 `platform_endpoints` 这一套：

| 变量 | 作用 | 指向 admin 时 |
| --- | --- | --- |
| `POINTER_API_BASE` | token / partner 接口根 | `http://<admin-host>:8788` |
| `POINTER_WEB_BASE` | 登录页根（拼 `/oauth/authorize`） | 同上 |
| `POINTER_OAUTH_CLIENT_ID` | 默认 `pointer-desktop` | 不用改，admin 已登记同名客户端 |

也就是说 admin 是「官网后端」的同构替代品，**不新增 `POINTER_ADMIN_*` 之类的客户端配置**，
也不需要改 `server/src/main.rs`。托管构建仍可把域名在编译期烧进去（`build.rs`）。

`pointer-contract` 只装 serde 类型与纯函数（`PlatformLoginCredentials`、`PlatformProviderTemplate`、
`model_catalog_hash`），三端共用同一份定义，字段名不可能漂移。
前端同理：`admin/web/` 与根 `src/` 各自构建，互不引用。

### 13.1 数据层：sqlx，三库可选

| 项 | 做法 |
| --- | --- |
| 驱动 | `sqlx` + `Any`，运行期由 `POINTER_ADMIN_DB_URL` 前缀决定：`sqlite:` / `postgres:` / `mysql:` |
| 占位符 | SQL 一律写 `?`；PG 下由 `AdminStore::sql` 改写成 `$1..$n` |
| 影响范围 | **只有 admin**；客户端与 server 仍用 rusqlite 的 `conversations.db`，不动 |
| SQLite | `max_connections=1` + `journal_mode=WAL` + `foreign_keys=ON`（单文件，单连接最省事） |
| 事务 | 先解析完角色 id 再开事务——SQLite 只有一条连接，事务内再查池会自锁 |

三条跨库 DDL 约束（否则 SQLite 能跑、MySQL 报错）：

1. 字符串主键用 `VARCHAR(n)`，不用 `TEXT`（MySQL 的 `TEXT` 不能做主键）；
2. 时间戳用 `BIGINT` 存 unix 秒，不用各库不同的日期类型；
3. 避开保留字：`meta_key` / `meta_value` / `role_key`，不写 `key`（MySQL 保留字）。

**第 4 条是实测踩出来的**：MySQL 上**所有** `TEXT` 列都读不出来。
sqlx 的 `Any` 驱动把 MySQL 的 `TEXT`（协议层就是 `BLOB`）映射成
`AnyTypeInfoKind::Blob`，而 `String` 只能从 `Text` 解码，于是报
`mismatched types; Rust type String is not compatible with SQL type BLOB`。
所以 DDL 里文本列写成 `TEXT$<n>` 标记，`schema::migrate` 按后端翻译：
MySQL → `VARCHAR(n)`，SQLite / PostgreSQL → `TEXT`（不引入长度上限）。

另外不用 `ON CONFLICT`、`INSERT OR IGNORE`、`RETURNING`——三者语法各不相同；
唯一性靠「先查后写」保证，索引重复创建的错误直接忽略。

| 层 | 位置 | 放什么 | 不放什么 |
| --- | --- | --- | --- |
| 领域 | `admin/src/*.rs`（平铺） | 表、迁移、密码、RBAC、授权、配额、凭证 | 任何 HTTP 概念 |
| 后台 API | `admin/src/http/` | 路由、cookie、状态码 | 业务规则、SQL |
| 控制面契约 | `admin/src/oauth/` | authorize / token / partner 七端点 | 管理后台逻辑 |
| UI | `admin/web/src/` | 展示与交互 | 权限判定 |

> 实现时没有建 `store/` 子目录：模块名（`users`/`sessions`/`rbac`/`resources`/`grants`/`usage`/`audit`）
> 已经能区分职责，多一层目录只是多一次 import 跳转。

测试：领域单测就近写在各自 `.rs` 的 `#[cfg(test)]`；端到端用 `admin/scripts/smoke.sh`
（真起进程、真发 HTTP、真换凭证）。

## 13.2 M0 实现进度（2026-11-07）

已落地并可运行（`cargo run -p pointer-admin`）：

| 能力 | 状态 | 入口 |
| --- | --- | --- |
| 账号 / 登录 / 会话 | 完成 | `POST /api/admin/auth/login`、`/api/admin/me` |
| RBAC（4 内置角色、25 权限） | 完成 | `GET /api/admin/roles`、`/api/admin/permissions` |
| 成员管理 + CSV 导入 | 完成 | `/api/admin/users*`、`POST /api/admin/users/import` |
| 统一资源（6 类）+ 授权 | 完成 | `/api/admin/resources*` |
| 密钥查看（单独权限 + 审计） | 完成 | `POST /api/admin/resources/:id/reveal` |
| 凭证组装 + 目录哈希 | 完成 | `admin/src/credentials.rs` |
| OAuth 七端点 | 完成 | `/oauth/authorize`、`/auth/app/token`、`/auth/partner/*` |
| 用量上报 + 配额（warn/block） | 完成 | `POST /auth/partner/token-usage`、`/api/admin/usage*` |
| 审计流水 | 完成 | `GET /api/admin/audit` |
| 前端工程 `admin/web/` | **完成**（6 页，hash 路由，产物由进程托管） | `npm run build` → `admin/web/dist` |
| 技能 zip 版本下发（控制面侧） | **完成**：上传/发布/下载 + 路径穿越校验 | `POST /api/admin/resources/:id/versions`、`GET /auth/partner/skills` |
| 技能 zip 版本下发（客户端侧 `org-skills`） | **完成**：目录 + provenance `org` + 后台同步 | `crates/pointer-core/src/skills/org_sync.rs` |
| 按角色下发档位 | **完成**：多条档位映射按「用户直授 > 角色 > 全员」取一条 | `admin/src/credentials.rs` |
| 配额拦截 | **完成**：`over_action=block` 超限 → `token_quota_exhausted=true`，客户端 `ensure_llm_allowed` 已有 gate | `admin/src/oauth/partner.rs` |
| 插件包下发（控制面侧） | **完成**：插件资源可传版本 + `GET /auth/partner/plugins` | `admin/src/oauth/partner.rs` |
| 插件包下发（客户端侧） | **完成**：按 `pluginId` 落盘 `~/.pointer/plugins`，只下发不激活 | `crates/pointer-core/src/skills/org_sync.rs` |

已知取舍：

- **授权码已落库**（`oauth_codes`，schema v5）：OAuth 登录是两个独立请求，
  多实例下会落到不同进程，放内存会「时好时坏」。库里只存 `sha256(code)`，
  一次性由原子 `UPDATE ... WHERE used_at IS NULL` 保证，并发重放只有一个能成功。
- **加密材料可切主密钥**：默认仍按 `machine_uid` 派生（单机，`enc:v1:`）；
  配置 `POINTER_ADMIN_MASTER_KEY` 后改用主密钥派生（`enc:v2:`），密文可跨实例/跨机解密。
  解密按前缀选钥匙，所以切模式后**老数据（v1）仍能在原机器上解**，新写入才走 v2。
  换错主密钥时接口报 500 并明确指出是 `POINTER_ADMIN_MASTER_KEY` 不一致，不会静默。
  启动日志会打印当前处于哪种模式。
- 技能包校验常量（20 MiB / 5 MiB / 条目数）与
  `crates/pointer-core/src/skills/external.rs` 各存一份——admin 不依赖 pointer-core，
  改一处必须同步另一处。
- 技能包存本地磁盘（`POINTER_ADMIN_DATA_DIR`，默认 `admin/data`），多实例部署前要换对象存储（用户已确认暂时保持本地）。
- 企业技能与插件**默认不启用**，但控制面可以按资源设「启用策略」（用户已确认只做两档）：
  | `activation` | 语义 |
  | --- | --- |
  | `optional`（默认） | 员工自己决定，下发不改变启用状态 |
  | `default_on` | 下发后默认启用；员工关掉后以他的选择为准，不再强开 |
  客户端落地：`sync` 写 `.org_policy.json`（`defaultOn` 列表），`manifest_to_skill` 标记
  `defaultEnabled`，`resolve_skill_ids` 在**没有 override** 时把默认启用项补进列表；
  前端 `agentDefaultSkillIds` 用同一规则，保证界面与运行时一致。
  员工一旦在面板里动过（写了 override），后续同步不再干预——**opt-out 就是 override 本身**，
  不额外记一份状态。
- **插件按策略下载**（用户选定方案 B）：
  | 情况 | 行为 |
  | --- | --- |
  | 插件 `default_on` | 后台同步自动下载落盘（仍不激活） |
  | 插件 `optional` | **不下载**，只在 `.org_catalog.json` 里列出来；员工在插件面板点「安装」才下 |
  | 技能（无论策略） | 全量下发（包小，且 agent 要能随时 `skill_read`） |
  目的：几十个插件不至于把员工磁盘占满。按需安装走 `org_sync::install_package`，
  明确绕开策略只下这一个。
- **插件即使设 `default_on` 也只落盘、不激活**：插件带工具、hooks、MCP，激活需要显式授权。
- **下发是客户端拉取（pull），不是服务端推送**：没有 webhook、没有长连接。
  客户端启动后 15 秒拉一次，之后每 6 小时一次；管理员发布新版本后，
  员工端最长等 6 小时（或重启客户端）才看到。暂无「立即同步」入口（用户已接受该频率）。
- **插件下发只落盘、不激活**：文件进 `~/.pointer/plugins`，是否启用仍走 `plugins/activation.rs`
  的既有流程。理由：插件带工具、hooks、MCP，「下发即激活」等于把执行能力直接塞给客户端。
- **插件资源必须带 `pluginId`**（payload 里 `pluginId` / `plugin_id` 均可）。缺了客户端直接拒绝下发，
  因为 `import_plugin_zip` 按插件自身 id 建目录，slug 对不上就无法判断目录归属、也无法安全覆盖/删除。
- 桌面端按公共客户端处理（无 client_secret，靠 PKCE）；server 是机密客户端。
- 余额恒为 `0.00`：控制面不接计费，`token_quota_exhausted` 由配额推导。
- `parentId` 之外的资源字段暂不支持修改（`kind`/`slug` 建后不可变）。
- PG / MySQL 已在真实实例上验证通过（PostgreSQL 16.15 / MySQL 9.3.0，见 13.1）。

## 13.3 真机数据库验证（2026-11-07）

| 后端 | 版本 | 结果 | 命令 |
| --- | --- | --- | --- |
| SQLite | 内存 / 文件 | 119 单测 | `cargo test -p pointer-admin` |
| PostgreSQL | 16.15 (Homebrew) | 通过 | `POINTER_ADMIN_TEST_DB_URL='postgres://127.0.0.1:5432/pointer_admin_test' cargo test -p pointer-admin --test db_backends` |
| MySQL | 9.3.0 (隔离实例，端口 3307) | 通过 | `POINTER_ADMIN_TEST_DB_URL='mysql://root@127.0.0.1:3307/pointer_admin_test' cargo test -p pointer-admin --test db_backends` |

冒烟测试做的是：连接 → 迁移 → **再迁移一次（幂等）** → 建用户 → 建会话 → 清理。

MySQL 那条一开始是失败的，暴露了 13 节里第 4 条约束（`TEXT` 在 `Any` 驱动下
被当成 `BLOB`）。修完 DDL 适配层才通过——**这条不真机跑是发现不了的**。

## 13.4 部署形态（单机 / 多实例）

### 单机（默认，什么都不用配）

```bash
cargo run -p pointer-admin
```

- 数据库：SQLite 文件（`POINTER_ADMIN_DB_URL=sqlite:...`）
- 密钥加密：按本机标识派生（`enc:v1:`）
- 包存储：`POINTER_ADMIN_DATA_DIR`（默认 `admin/data`）
- 适用：员工规模一两千以内、不要求高可用

### 多实例

**三条硬要求，缺一条就会出随机故障：**

| # | 要求 | 不满足会怎样 |
| --- | --- | --- |
| 1 | **同一个数据库**，且必须是 PostgreSQL / MySQL | SQLite 是单机文件，多进程/跨机写会锁冲突 |
| 2 | **同一个 `POINTER_ADMIN_MASTER_KEY`** | 各实例派生出不同密钥，A 写的 key B 解不开（接口 500） |
| 3 | **同一个 `POINTER_ADMIN_DATA_DIR`**（共享卷 NFS/EFS） | 上传落在 A 的磁盘，下载打到 B 就 404 |

```bash
# 每个实例的环境变量（除地址外完全一致）
POINTER_ADMIN_ADDR=0.0.0.0:8788
POINTER_ADMIN_DB_URL=postgres://user:pass@db.internal:5432/pointer_admin
POINTER_ADMIN_MASTER_KEY=<同一个 32+ 字节随机串，放进 secret manager>
POINTER_ADMIN_DATA_DIR=/mnt/pointer-admin-data      # 共享卷
POINTER_ADMIN_OAUTH_SECRET=<同一个>
```

**部署后自查**（任一实例执行，结果必须一致）：

```bash
# 1. 加密模式：日志里应有「使用 POINTER_ADMIN_MASTER_KEY（可跨实例解密）」
# 2. 包可见性：在 A 上传一个技能包，然后直接打 B 的下载地址
curl -s -o /dev/null -w '%{http_code}\n' -H "authorization: Bearer $TOKEN" \
  "http://<实例B>/auth/partner/skills/<slug>/<version>/download"   # 期望 200
# 3. 登录：连续登录 20 次（负载均衡会把两个请求打到不同实例）
```

### 从单机迁到多实例

1. **先在原机器上**配好 `POINTER_ADMIN_MASTER_KEY` 并重启一次——此后新写入的密钥是 `enc:v2:`，可跨机解密；
2. 把数据目录挂成共享卷；
3. 加第二个实例。

**老数据的坑**：切换前写入的 `enc:v1:` 密文**只有在原机器上才解得开**。
所以「先切 master key、再迁数据」是唯一安全顺序。如果已经换了机器，
只能把企业密钥重新录入一遍（`POST /api/admin/resources/:id` 带 `secret` 字段）。
后续可以加一个「按当前模式重新加密所有密文」的维护接口来消除这个手工步骤，目前没有。

### 已知限制（多实例下仍然成立）

| 限制 | 影响 | 兜法 |
| --- | --- | --- |
| 包存本地盘 | 不挂共享卷就只有上传的那台能下载 | 挂共享卷；后续可换对象存储 |
| 没有「立即同步」入口 | 发布新版本后员工最长等 6 小时 | 重启客户端（15 秒后拉一次） |
| 审计/用量写库 | 无 | — |

## 14. 相关文档

- [control-plane-and-editions.md](control-plane-and-editions.md)（三种用法、客户端/服务端、门禁）
- [../developer/standalone-local-login.md](../developer/standalone-local-login.md)（本地账密现状）
- [../developer/standalone-deployment.md](../developer/standalone-deployment.md)（服务端部署）
- [../developer/session-user-id.md](../developer/session-user-id.md)（`ListScope` 与多用户隔离现状）
- [../developer/skills-persistence.md](../developer/skills-persistence.md)（技能三级库）
