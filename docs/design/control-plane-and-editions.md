# Pointer 客户端 / 服务端：账户与控制面改造

状态：P0（门禁与可见性）已落地；P1（可绑定的控制面）待评审。  
仓库：agent-pointer。产品名：Pointer。  
官网控制面后续单独开源；本仓库只消费其 HTTP API。

本文是改造设计，不是用户教程。
对外文案用「三种用法」；打包用「客户端 / 服务端」；
`official` 只作为打包口味出现在构建说明里。

## 1. 为什么改

开源后要同时满足：

- 官网个人：登录即用，本地 Agent 和云主机挂在同一账户
- 本地个人：不登录也能在本机用自己的模型 Key；需要时再连官网或自建控制面
- 公司：自己部署，统一登录；可以只要一台 server，也可以再要云主机

现状把「必须登录 Pointer 账户」写进了桌面发送和 core 门禁。
未绑定的构建又把官网地址默认留空，结果是：既登不上，也不能只靠本地 Key 对话。
云主机代码还在，文档却写成未绑定没有购买入口。

目标：官网能力留在同一棵树里，用绑定和门禁区分用法，不要按构建删功能。

## 2. 对外：三种用法

用户只答两件事：**用谁的账户**，**Agent 跑在哪**。
不要对外讲「官网 × 个人 × 企业 × 本地 × 云端」这类叠加枚举。

| 用法 | 账户 | 用户装什么 | Agent 在哪 |
| --- | --- | --- | --- |
| **用 Pointer** | Pointer 账户 | 官方客户端 | 自己电脑，或 Pointer 云主机 |
| **自己用** | 不登录 | 本地客户端（未绑定）；要用浏览器再加服务端 | 这台机器（或自己起的 server） |
| **公司部署** | 公司账户 | 公司装服务端；员工用浏览器或公司发的客户端 | 公司机器或公司云主机 |

「自己用」里勾选 Pointer 账户，不是第四种产品，只是绑到官网控制面。
「公司部署」也不是第三种构建，是公司提供入口和账户。

设置和文档的提问顺序：

```text
用谁的账户？
  不登录 → 自己用
  Pointer 账户 → 用 Pointer
  公司账户 → 公司部署

Agent 跑在哪？
  这台电脑 / 公司服务器 → 本地
  云主机 → 云端
```

## 3. 打包：客户端 / 服务端

本仓库只打两种产物。`official` 是唯一打包口味，不是第三种包。

| 产物 | 命令 | 职责 |
| --- | --- | --- |
| **客户端** | `npm run tauri:build` | 桌面 App。本机 Agent 在这个进程里，不依赖 pointer-server。 |
| **服务端** | `npm run server:build` | pointer-server + 同一套 Web 界面。浏览器或云主机用。 |

登录站、店铺、计费在官网控制面仓库，这里不打。

| 口味 `POINTER_EDITION` | 客户端 | 服务端 |
| --- | --- | --- |
| `official` | 预填官网域名；自动更新 | standalone 要 License |
| 未设置（personal） | 未绑定；无更新 | 不要 License |

发布时按需打格子，不要说「一个官方包 / 一个本地包」：

```text
官方客户端     「用 Pointer」的默认安装
本地客户端     「自己用」的默认安装（未绑定）
官方服务端     官网云主机镜像；官方签发的 standalone
本地服务端     「自己用」的浏览器入口；「公司部署」的默认安装
```

「用 Pointer」的用户通常只装官方客户端，不装服务端。

## 4. 原则

1. 产物、口味、控制面绑定、服务端认证方式，四问正交，不合成一个枚举。
2. 控制面是运行时绑定，不是编译开关。登录、充值、云主机、余额走同一套代码。
3. 发消息看凭证：本地 Key 和控制面会话都合法。官方客户端可额外要求登录。
4. 公司云主机走控制面 OAuth，不走 standalone。`?sso=` 只服务「只要 server、无店铺」。
5. 不增加 `POINTER_EDITION=enterprise`。不新增长期商业分支。
6. 不改产品名、bundle id、crate、数据目录。
7. 桌面和 Web、三个桌面系统，同一套门禁和绑定。
8. 分支和异常要打日志；该失败的对用户抛错，不静默。

## 5. 四个正交概念

```text
产物                     客户端 或 服务端
POINTER_EDITION          该产物口味：默认域名、客户端更新、服务端 License
control_plane binding    当前进程是否有可用的 web_base + api_base
deployment_mode          仅服务端：platform（控制面 OAuth）
                         或 standalone（账密 + 门户短时票）
```

客户端没有 `deployment_mode`。
客户端要么仅本地，要么当控制面的 OAuth 客户端。
不要用写死的 `authMode = platform` 表示「这是桌面」。

### 5.1 口味只决定默认值和商业约束

| | official | 未设置 |
| --- | --- | --- |
| 默认控制面 | 编译期常量 readflowai.com | 空（未绑定） |
| 客户端自动更新 | 开 | 关（personal 构建） |
| 用量上报默认 | 已绑定且非 standalone 时开 | 关 |
| 服务端 standalone License | 要 | 不要 |
| 登录 / 云主机 / 余额 / 充值代码 | 保留 | 保留 |

官方域名可以留在源码当常量。未设置时不写入绑定。

### 5.2 控制面绑定

解析顺序（先命中先用；空串当未设置）：

1. `POINTER_API_BASE` / `POINTER_WEB_BASE` / `VITE_POINTER_WEB_BASE`
2. 用户设置（P1）
3. 构建默认：official 用常量，未设置则为空

已绑定：`api_base` 与 `web_base` 都非空。

P1 设置项（面向用户，不写开发词）：

- 不使用网上账户
- Pointer 官网（填入官方常量，须用户主动选）
- 自定义（公司或预览环境）

云主机购买、列表、打开：已绑定才可调用。
未绑定：页可以开，说明先绑定，请求打 warn，不假装成功。

### 5.3 服务端认人

| `deployment_mode` | 身份来自 | 用于 |
| --- | --- | --- |
| `platform` | 控制面 OAuth（官网或公司自建官网） | 用 Pointer；自己用但已绑控制面；公司还要云主机 |
| `standalone` | 本机账密或门户 `?sso=` | 公司只要 server；完全离线的自建 Web |

standalone 关闭云主机 OAuth 和店铺。
公司既要统一登录又要云主机：部署开源后的控制面，服务端用 `platform`，客户端绑定该公司地址。

两套登录不要合成一种 SSO：

```text
公司只要 server     门户短时票 → standalone 服务端
公司还要云主机      公司 IdP → 控制面 → 现有 OAuth
```

## 6. 三种用法落到实现

| 用法 | 默认产物口味 | 控制面 | 认证 |
| --- | --- | --- | --- |
| 用 Pointer | 官方客户端（`official`） | 默认官网 | 登录 Pointer 账户 |
| 自己用 | 本地客户端（未设置） | 未绑定 | 不登录，本地 Key |
| 自己用，勾选官网 | 本地客户端（未设置） | 用户绑定官网 | 与「用 Pointer」同一套登录和店铺 |
| 公司只要 server | 本地服务端（未设置） | 未绑定 | standalone 账密或短时票 |
| 公司还要云主机 | 本地客户端 + 公司控制面 | 绑定公司控制面 | 控制面上的统一登录（官网开源后） |

## 7. 发消息门禁

桌面发送 / 附件与 core 对话入口必须同一规则，禁止只改一端。

```text
has_local_llm         设置里已有可用 API Key
has_identity          控制面已登录，或 standalone 本机/SSO 会话
control_plane_bound   已绑定控制面
```

| 条件 | 结果 |
| --- | --- |
| 有身份（控制面会话，或 standalone 本机/SSO 会话） | 允许 |
| 无身份，有本地 Key，且未绑定控制面（standalone） | 允许 |
| 已绑定控制面且无身份 | 拒绝：请先登录 Pointer 账户 |
| 未绑定且无本地 Key | 拒绝：自己用去填模型；用 Pointer 去登录 |
| 有控制面身份且非 standalone | 现行余额门禁；充值页用绑定的 web_base |
| 自动化触发 | 有本地 Key 可走；否则提示登录或填 Key |

用量上报和余额请求：仅当已绑定、已登录、且非 standalone。
未绑定不得打官方余额接口。

## 8. 分阶段

每期可单独合入、单独回滚。不依赖改名或拆仓库。

### P0 门禁与可见性

让「自己用」在未绑定客户端上成立；「用 Pointer」行为不变。

改动：

- core 对话入口：未绑定且有本地 Key → 允许未登录
- 桌面 / Web 发送与附件：同一规则；抽成两端共用的一处判断，core 与前端各测
- 云主机入口：看是否已绑定，不看「是不是桌面且非 standalone」
- 未绑定仍打店铺：warn + 界面说明
- 用户 / 贡献者文档与本文一致：未绑定即 standalone，不是没有云主机代码

不做：设置里填控制面；改 License；改 updater。

验收：

- 未绑定客户端、不设环境变量、已填模型 Key：能发消息
- 官方客户端、未登录：不能发消息
- 未绑定客户端：云主机页可开，不能买，不请求官方域名
- 未设口味的本地开发：与今天一致

### P1 可绑定的控制面

让「自己用」能勾选 Pointer 账户或公司地址，不用改环境变量、不用重编。

- 设置增加网上账户三项；写入用户设置；参与第 5.2 节解析
- 绑定后登录、充值、云主机走现有平台认证和店铺接口
- 环境变量仍覆盖设置
- 桌面 OAuth 成功页仍跳到 `{web_base}` 上现有约定 query

验收：

- 未绑定客户端选 Pointer 官网后，登录和买云主机与官方客户端相同（官网在线时）
- 自定义地址后，请求只打该域名
- 改回不使用网上账户后，店铺不可用，本地 Key 对话仍可用

### P2 公司只要 server

standalone 账密和 `?sso=` 已有。本期只收口预期：

- 员工从浏览器打开公司服务端完成统一登录
- 不把短时票接到桌面回环
- 文档写清：只要 server 就用 standalone，不要配店铺

### P3 公司还要云主机（依赖官网开源）

本仓库 P1 的自定义绑定已够。
IdP、组织、店铺、计费在控制面仓库做。

- 公司部署开源控制面
- 云主机上的服务端用 `platform`
- 客户端绑定公司 `web_base` / `api_base`
- 本仓库继续现有 OAuth 换票，不发明第三种登录

## 9. 明确不做

- 不为未绑定构建去掉登录、云主机、充值
- 不从源码删除官方默认域名
- 不给 standalone 加店铺来凑公司云主机
- 不新增长期商业分支
- 不在提示词里写文件名或开发注释
- 界面文案不写 edition、控制面、deployment_mode 等开发词

## 10. 跨端与跨平台

门禁和绑定解析桌面、Web 共用。
Web 上的前端环境变量只影响展示用链接；真正请求以服务端环境变量和设置为准。
OAuth 回环仅客户端；Web 继续重定向。
Linux / Windows / macOS 同一套设置项。

## 11. 可观测性

| 事件 | 级别 |
| --- | --- |
| 解析后的产物角色、口味、deployment_mode、是否绑定（可打域名，不打密钥） | info |
| 门禁放行：`local_llm` / `control_plane_session` / `standalone_session` | info |
| 门禁拒绝原因 | warn |
| 未绑定仍打店铺或余额 | warn |
| 绑定失败、换票失败 | warn，必要时对用户抛错 |

## 12. 与现状差距

P0 已落地：`standalone` 由「是否绑定控制面」推导，客户端与服务端同义；发消息门禁统一为
「有身份 ∨（有本地模型 Key ∧ 未绑定）」；设置入口在 standalone 下开放；桌面认证模式不再写死
为 platform；未绑定时不再尝试刷新平台会话。

已知遗留（待后续专项）：

- **未绑定实例仍展示上次绑定缓存的控制面模型目录**（`{data_dir}/platform_model_catalog.json`）：
  目录里的服务商可以自带 Key 使用，但界面上会出现「有十几个模型却一个都用不了」的观感。
  方案待定（保留并标记来源 / 未绑定时清空 / 两者结合）。
- **Web 端拿不到 providers 的 `apiKey`**（后端下发时会剥离密钥），输入框门禁只能退回合并后的
  `hasKey`（默认服务商口径），在「默认服务商无密钥、其他服务商有」时仍可能误拦。
  修法：后端透出按「目标 + 档位」解析后的 provider，供前端门禁复用。

## 13. 随各期同步的文档

- [../contributing/editions.md](../contributing/editions.md)
- [../contributing/cross-platform-build.md](../contributing/cross-platform-build.md)
- [../user/editions.md](../user/editions.md)
- [../user/cloud-host.md](../user/cloud-host.md)
- [../user/standalone-server.md](../user/standalone-server.md)
- [../developer/standalone-deployment.md](../developer/standalone-deployment.md)
- [../developer/architecture.md](../developer/architecture.md)
- [../developer/desktop-oauth-web-integration.md](../developer/desktop-oauth-web-integration.md)
