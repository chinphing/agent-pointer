# 配置界面重构 — 实现方案

> 分支：`refactor/settings-ui`（独立 worktree 已拆除；设置页以 `main` 为准）
> 基线：`main @ 418d36f1`
> 状态：已评审并完成界面迭代（2026-08-08）：O4 明确 = 平台配置由 **pointer-official 官网 API** 下发；设置已调整为全屏、余额仅账户页、豆包归平台、调试仅保留请求保存。

## 1. 目标

1. **设置不再弹窗**：设置改为应用主界面的一等页面，通过「返回」回到主聊天界面。
2. **突出账户余额与充值入口**：仅在设置 → 账户中展示余额，并提供充值入口。
3. **LLM 配置简化**：只区分「平台」+「自定义」两类；每类仍分三档（快速/标准/高级）不变，优化每个档位的设置界面。
4. **平台配置不固化在代码里**：平台信息（服务地址、模型清单、默认参数、三档模型映射）从平台官网/后端加载，默认值由平台侧设置。

实现要求：分期实施（先界面、后逻辑），不降低风险，独立 worktree 开发。

---

## 2. 现状分析（基线调查结论）

### 2.1 设置弹窗

| 位置 | 说明 |
|---|---|
| `src/App.vue` | `showSettings` 控制设置页面与聊天视图切换；`<SettingsDialog>` 全屏呈现 |
| `src/components/settings/SettingsDialog.vue` | 全屏设置页：标题栏、左侧分区导航、右侧内容区与按需保存操作 |
| 分区 | `account` 账户 / `automation` 自动化 / `channels` 连接 / `skills` 技能 / `assistant` 智能体（含模型与档位） / `generation` 界面配置 / `debug` 调试 / `cloud` 云主机（桌面·平台模式）/ `about` 关于 |
| 侧栏分组 | 按职责分 4 组（无标题，组间用分割线）：账户 ｜ 智能体与配置（智能体/模型配置/系统设置）｜ 自动化与集成（自动化/连接/技能）｜ 系统（调试/云主机/关于）；`debug`/`cloud` 可见性规则不变。模型配置是独立一等分区，承载 `ModelServiceSection.vue` 的平台服务、自定义服务和场景档位映射。 |
| `src/components/layout/AppShell.vue` | 主布局（侧栏+主区）；侧栏有「自动化/技能/连接」入口，通过 `emit('open-settings', section)` 打开设置并定位分区 |

关键结论：设置是 `v-if` 渲染的覆盖层，主界面（ChatView）始终在下面。改成页面形态时，需要把「显示/关闭」改为「视图切换」，并保留 `initialSection` 深链能力。

### 2.2 账户余额与充值

| 位置 | 说明 |
|---|---|
| `src/lib/platformUrls.ts` | `platformBillingUrl()` → `https://pointer.readflowai.com/profile/billing`；`openPlatformBillingPage()` |
| `src/components/settings/panels/CloudSettingsPanel.vue` | 云主机页显示 `me.balance_yuan`（来自 `getCloudPlatformMe()`，`lib/cloudAgents.ts`），有「充值」按钮 |
| `src/stores/platformAuth.ts` | `session`（含 `tokenQuotaExhausted`）、`markTokenQuotaExhausted()`；**没有余额字段** |
| 聊天错误态 | `AgentMessageBody` / `AssistantErrorMessage` / `Composer` 在余额耗尽时显示「去充值」 |

关键结论：余额数据已有来源（`getCloudPlatformMe`），但只在「云主机」设置页展示，主界面不可见。需要把余额提升到主界面常驻位置，并支持余额耗尽/低余额提醒。

### 2.3 LLM 配置（现状）

| 位置 | 说明 |
|---|---|
| `src/components/settings/ProviderSettingsPanel.vue` | 「模型服务」面板（调试模式可见）：服务商列表、添加/编辑（服务类型 千问/深度求索/OpenAI兼容、ID、名称、API地址、密钥、模型列表、服务商级参数 + 每个模型 同上/定制 + 模型参数弹窗） |
| `src/components/settings/RuntimeParamsForm.vue` | 参数表单：最大输出、深度思考/思考预算（千问）、推理力度（深度求索）、创造性、回传推理、扩展参数 extra_body |
| `src/components/settings/ModelCapabilityForm.vue` | 模型能力：视觉/生图/生视频开关 |
| `src/components/settings/panels/AgentSettingsPanel.vue` | 「智能模式」面板（调试模式）：各智能体默认模型；各模式对应模型（快速/标准/专家）；电脑操控各档模型（快速/标准/高级）；图片/语音/视频各模式模型；computer pipeline |
| `src/lib/providerParams.ts` | **硬编码**平台模板 `PROVIDER_TEMPLATE_OPTIONS`（qwen/deepseek/openai_compatible 的默认 ID/名称/baseUrl/模型清单），`isQwenProvider`/`isDeepSeekProvider` 按 ID/域名启发式判断 |
| `src/stores/settings.ts` | **硬编码** `defaultPlatformSettings()`：默认服务商、默认模型、温度/最大输出、`agentModeLlm`/`mediaModeLlm`/`computerTierLlm` 三档映射、`mediaModelOverrides` |
| `src/lib/modelCapabilities.ts` | **硬编码**模型能力推断（视觉/生图/生视频/语音）与生成模型清单（qwen/doubao 系列） |
| 运行时 | `useRuntimeParams.ts` 按「服务商默认 → 模型定制」两级取参；`isQwenProvider`/`isDeepSeekProvider` 决定表单变体 |

「三档」现状：`PerformanceMode = fast|standard|expert`（快速/标准/专家）用于 `agentModeLlm` 与 `mediaModeLlm`；`ComputerTierKey = primary|intermediate|advanced`（快速/标准/高级）用于 `computerTierLlm`。三档映射目前只在**调试模式**的表格里可改，且模型下拉依赖 `s.allModels`（即 providers 的 models 并集）。

关键结论：
- 平台配置三处硬编码（providerParams / settings 默认值 / modelCapabilities）是第 4 目标要移除的。
- 三档配置散落在调试面板的多张表格中，交互不直观，是第 3 目标要优化的。

### 2.4 设置存储与接口

| 位置 | 说明 |
|---|---|
| `src/lib/api.ts` / `tauri.ts` / `web.ts` | `getSettings` / `updateSettings` / `updateAgentSettings` / `updateDebugSessionSettings` / `updateUserSettings` / `updatePlatformSettings` / `setApiKey` 等 |
| `server/src/main.rs` | 桌面/本地服务：`/api/settings` GET/PUT 等；standalone 模式从 `pointer-server.toml` 应用 LLM providers |
| 平台模式 | 平台后端（pointer.readflowai.com）返回 `EffectiveSettingsView { user, platform, merged, canEditPlatform, isPlatformAdmin }` |

关键结论：平台模式后端已能下发 `platform.providers` 等，但前端仍用硬编码默认值兜底。第 2 期需要新增「平台目录」数据（服务商模板 + 模型三档 + 能力标注），并让默认值完全由平台侧下发。

### 2.5 pointer-official 官网现状（第 2 期对接基础）

**架构**：`apps/web`（Next.js 官网/控制台）+ `apps/api`（FastAPI :8001）+ `infra`（Postgres/Redis）。桌面端 Release 默认 `https://pointer.readflowai.com`（web）/ `https://pointer-api.readflowai.com`（api）。

**已有设施（与本需求直接相关）**：

| 设施 | 位置 | 说明 |
|---|---|---|
| 登录下发 LLM 凭据 | `apps/api/app/routers/app_oauth.py` → `build_oauth_llm_payload`（`services/user_llm_capability.py`） | `POST /auth/app/token` 返回 `api_key` / `llm_provider` / `llm_source` / `provider_api_keys` / `media_oss` |
| 平台密钥池 | `services/platform_provider_llm.py` | `PlatformProviderLlmKey`（qwen/deepseek/doubao 多 TOKEN 随机选取）；`PlatformNoviceLlmConfig` 旧版迁移 |
| 用户自有密钥 | `routers/me_llm_api_keys.py` | `/api/me/llm-api-keys` CRUD + `/reveal` 一次性取明文；`UserLlmApiKey` 表 |
| 供应商规范化 | `schemas.py` | `normalize_llm_provider`（qwen/deepseek/aliyun_qwen→qwen/doubao，支持自定义）、`llm_provider_label_zh` |
| 计费/余额 | `services/billing.py`、`services/balance_credit.py`、`routers/me_llm_api_keys.py` | `/api/me/balance-ledger`、`/auth/partner/balance`（run_chat 门禁）、充值（微信支付） |

**pointer-app 侧消费链路**：
- 桌面/云上 server 登录后：`crates/pointer-core/src/platform_config.rs` `apply_login_credentials_to_model_settings` / `app_state.rs` `apply_login_credentials` → 把登录下发的 key 注入 `platform_config.providers`（按 `resolve_llm_provider_id` 匹配 provider）。
- **但 provider 的 baseUrl/模型清单/默认参数仍来自硬编码**：Rust `src-tauri/src/models.rs` `PlatformSettings::default()`（千问/OpenAI/本地/深度求索）+ 前端 `providerParams.ts` / `settings.ts` / `modelCapabilities.ts`。官网 API 目前**不下发** provider 目录（baseUrl/模型/三档/能力），这是第 2 期要补的核心缺口。

---

## 3. 目标架构

```
应用根视图
 ├─ chat 视图（AppShell：会话/项目侧栏 + 主区）
 └─ settings 视图（全屏覆盖 AppShell，带返回按钮回到 chat）
      ├─ 分区导航
      ├─ 账户（余额卡片 + 充值入口，仅此处展示）
      ├─ 模型服务（平台 + 自定义；平台包含千问/DeepSeek/豆包）
          │    ├─ 平台：官方预置，仅填密钥 + 三档参数（数据来自平台目录）
          │    └─ 自定义：完整 OpenAI 兼容服务商编辑（现有表单）
          └─ 其余分区（自动化/连接/智能体/云主机/关于）
```

### 数据流（第 2 期目标态）

```
平台官网/后端 ──GET /api/platform-catalog──▶ 前端 PlatformCatalogStore
                                              ├─ providers 模板（id/名称/baseUrl/模型/能力/三档映射/默认参数）
                                              └─ 默认值（activeProviderId/model/temperature/...）
前端只保留：用户自定义服务商（openai_compatible）+ 用户密钥/覆盖
```

---

## 4. 分期计划

### 第 1 期：基础界面改造（UI）

**范围：不改变数据模型与保存链路，纯视图/导航/交互重构。**

#### 4.1 设置从弹窗改为页面

1. `App.vue` 引入视图状态：`mainView: 'chat' | 'settings' | 'skills'`（或最小改动：保留 `showSettings`，但改为渲染在 AppShell 主区内部）。
   - 推荐：`AppShell` 增加 `view` prop / 具名 slot，主区在 `chat` 与 `settings` 之间切换；`SettingsDialog.vue` 改造成 `SettingsView.vue`（去掉 fixed overlay/backdrop，容器改为占满主区，头部 X 改为「← 返回」）。
2. 保留深链：`openSettings(section)` 仍接受 section；侧栏「连接/自动化」等入口行为不变（只是从弹窗切换变成页面切换）。
3. 返回逻辑：
   - 侧栏顶部「返回对话」：`@close → showSettings=false`；
   - 所有分区均即时保存；返回按钮是唯一退出入口，不区分「保存/取消」。
   - 当前未实现 Esc 返回；如后续补充，需先定义输入框焦点、编辑弹窗与未完成操作的优先级。
4. 调试菜单（provider/generation/agent 分区）保持可见性规则不变，只是从弹窗分区变为页面分区。
5. 技能管理作为设置内嵌分区：左侧「技能」直接切换右侧内容区，不再打开独立弹窗；技能启用、搜索和 zip 导入均即时保存。
6. 「系统设置」为普通用户可见分区：工具调用显示（sidecar/非 sidecar/卡片/结果）、显示推理过程、任务板面板、子 Agent 边框面板、子任务板及运行环境设置。原始内容查看、标记截图查看和保存每轮对话请求属于调试分区，仅平台管理员或 standalone 本地管理员可见。

**验收**：全屏/窗口下设置以页面形态呈现；打开设置不遮挡聊天；返回后聊天状态无损；深链分区定位正常；所有分区修改即时持久化，返回按钮为唯一退出入口。

#### 4.2 账户页突出余额与充值（已调整）

1. 数据：`usePlatformBalance` 在登录后调用 `getCloudPlatformMe()` 取 `balance_yuan`，余额耗尽后重取；低余额阈值为 10 元。
2. UI：余额仅在设置 → 账户页显示，作为首要信息突出展示，并提供低余额/耗尽提示和「充值」按钮（`openPlatformBillingPage()`）；主界面不展示余额。聊天错误态「去充值」保持不变。
3. 兼容：余额接口仅桌面 Tauri 平台模式可用；Web 与 standalone 隐藏该卡片，拉取失败静默降级。

**验收**：桌面平台登录后在账户页可见余额与充值入口；主界面无余额；点击充值打开平台账单页。

#### 4.3 LLM 配置简化（平台 + 自定义；三档 UI 优化）

1. 「模型服务」从调试模式提升为一等分区（不再依赖 debug 开关；`canEditPlatform` 等权限语义保持）。
2. 分区内分成两个页签：
   - **平台**：列出平台预置服务商（第 1 期仍读 `PROVIDER_TEMPLATE_OPTIONS`，第 2 期改为平台目录）。每个服务商卡片：名称/地址/密钥状态；点击展开仅需配置：
     - API 密钥；
     - 三档（快速/标准/高级）各档模型与参数 —— 用卡片式（现有「智能模式」表格重构），每档一行：模型下拉 + 深度思考开关 + 思考预算/推理力度（按平台类型显示对应变体）；
   - **自定义**：保留现有 OpenAI 兼容服务商完整编辑（ID/名称/API地址/密钥/模型列表/服务商参数 + 每模型 同上/定制）。
3. 三档编辑 UI 优化（新组件 `TierModelRows.vue` 或重构现有表格）：
   - 按「类型」分组：对话（agent）、图片理解、语音理解、视频理解、电脑操控；
   - 每类型三档卡片：档位说明（快速/标准/高级的用途文案）+ 模型下拉（按类型过滤能力：视觉/语音）+ 思考开关/预算；
   - 默认值标注「平台默认」，改动后显示「已覆盖」徽标（便于识别与恢复默认）。
4. 数据字段第 1 期保持：仍写回 `agentModeLlm` / `mediaModeLlm` / `computerTierLlm` / providers；当前保存链路统一为 `saveModelService` / `saveAgentPreferences` → `updateUserSettings`，重启后保留。`saveDebugSession` 仅为旧 API 兼容和测试保留，设置页面不调用。
5. `AgentSettingsPanel.vue` 中的旧三档表格已不再接入设置页；后续单独清理或改为受控只读摘要，避免恢复第二套编辑入口。
6. 删除语义：仅「自定义服务」显示删除按钮；删除后立刻提交完整服务列表到用户设置，持久化失败时恢复删除前状态。平台服务由平台统一管理，不提供删除入口。

**验收**：非调试模式下普通用户也能配置模型服务；平台服务商只需密钥+三档；自定义服务商完整编辑不受影响；三档改动保存后运行时生效（用 `runTest` 或实际对话验证）。

### 第 2 期：配置逻辑改造（数据/后端）

**范围：移除平台配置硬编码，默认值平台化。**

#### 4.4 平台目录下发（数据源 = pointer-official API）

**目标**：平台配置（服务商 baseUrl/模型清单/能力标注/三档默认/全局默认）由官网 API 下发，代码（前端 + Rust）不再固化；默认值在平台侧设置。

**官网 API 侧（pointer-official，新增/扩展）**

1. 新增平台目录接口（建议 `GET /api/llm/catalog`，公开或 partner 鉴权均可，数据含模型清单与默认值，不含密钥）：
   ```ts
   interface PlatformLlmCatalog {
     version: string                       // 目录版本，客户端缓存用
     providers: Array<{
       id: string                          // qwen / deepseek / doubao / custom...
       name: string
       baseUrl: string
       variant: 'qwen' | 'deepseek' | 'doubao' | 'generic'
       models: Array<{ id: string; capabilities: { vision?; image?; video?; audio? } }>
       defaultParams: { temperature?: number; maxTokens?: number }
     }>
     tierDefaults: {                       // 三档默认映射（快速/标准/高级）
       agentModeLlm: Record<string, Record<PerformanceMode, TierModelRef>>
       mediaModeLlm: Record<'image'|'audio'|'video', Record<PerformanceMode, TierModelRef>>
       computerTierLlm: Record<ComputerTierKey, TierModelRef>
     }
     defaults: { activeProviderId; model; temperature; maxTokens; mediaModelOverrides }
   }
   ```
2. 管理入口：平台管理员在官网后台维护目录（数据表 + 管理 API）；默认值「在平台设置」= 目录中的 `defaults`。
3. 复用现有设施：密钥仍走 `provider_api_keys`（用户自有 `/api/me/llm-api-keys`）或平台密钥池；目录接口只管「形状/默认值」，不碰密钥。

**pointer-app 侧**

1. 新增 `platformCatalog` 拉取/缓存（Rust 侧：登录时随凭据一起拉取并写入 `platform_config`；前端：`settings.load()` 后拉取供 UI 渲染）。
2. 删除硬编码，改为目录驱动：
   - 前端 `providerParams.ts`：`PROVIDER_TEMPLATE_OPTIONS` 改为来自目录；`isQwenProvider`/`isDeepSeekProvider` 启发式改为目录 `variant` 标记（保留旧逻辑作兼容回退）。
   - 前端 `settings.ts`：`defaultPlatformSettings` 的模型默认值/三档映射改从目录 `defaults`/`tierDefaults` 生成；本地仅保留「自定义 OpenAI 兼容」空模板。
   - 前端 `modelCapabilities.ts`：模型清单与能力推断改读目录 `models[].capabilities`（保留按名称推断作为自定义服务的回退）。
   - Rust `models.rs`：`PlatformSettings::default()` 的 provider 列表改为由目录/`pointer-server.toml` 填充，不再内嵌模型清单。
3. standalone 模式：目录不可用时回退 `pointer-server.toml`（LLM providers）+ 最小内置兜底（OpenAI 兼容空模板），应用照常启动并提示「平台目录不可用」。
4. 兼容与迁移：
   - 旧用户已有 providers/三档覆盖保留（目录只补默认，不覆盖用户显式配置）；
   - 目录变更（模型上下架）不破坏已保存配置（缺失模型时提示并回退到同档默认）；
   - 平台模式下新增服务商/改三档继续通过 `updateUserSettings` 保存用户覆盖；平台目录仅下发默认形状与默认值，不能覆盖用户显式配置。

**验收**：修改平台目录（后端）后，前端无需发版即可看到新平台/模型/默认值；旧配置不回退、不丢失；无目录可用时应用仍可启动并提示。

---

## 5. 风险与兼容性

| 风险 | 对策 |
|---|---|
| 设置页改造影响启动性能 | 设置视图继续懒加载（`defineAsyncComponent`），首帧不加载 |
| 页面形态下保存/取消语义变化 | 各分区即时保存，无底部保存栏；返回按钮为唯一退出入口 |
| 余额接口在 web/standalone 不可用 | 按运行环境降级隐藏；错误静默，不阻塞 UI |
| 三档配置迁移到新页可能影响运行时 | 第 1 期保持数据字段与保存链路不变，仅改 UI 组织；迁移后用现有 `runTest`/对话验证 |
| 平台目录缺失/变更导致默认值漂移 | 目录加载失败用当前本地值兜底；目录变更不覆盖用户显式配置 |
| 删除硬编码可能破坏 standalone 模式 | 保留 `pointer-server.toml` 作为 standalone 的配置源，前端目录改为「平台模式=远程 / standalone=配置+兜底」 |

## 6. 测试策略

- 组件测试：`SettingsView` 视图切换、余额胶囊状态、三档编辑器组件（沿用 vitest 结构）。
- 手动验收清单（第 1 期）：打开设置→返回；深链分区；保存后返回；余额显示/充值；平台+自定义两类配置；三档修改后对话生效。
- 第 2 期：模拟目录接口返回不同数据验证渲染；旧配置迁移用例。

## 7. 开放问题（已确认）

- **O1**（更新）：余额仅在桌面平台模式的设置 → 账户中展示，并作为首要信息突出；standalone 与 Web 隐藏。数据源为 `getCloudPlatformMe`，拉取失败静默降级。
- **O2**（更新）：设置页与 AppShell 互斥渲染，覆盖整个应用；聊天与工作区侧栏均不保留，返回回到聊天。macOS 设置标题栏保留原生红绿灯安全区。
- **O3**（更新）：平台服务商为千问、DeepSeek、豆包；第 1 期每个平台保持单实例（密钥可换），多实例归入自定义。
- **O6**（新增）：普通“打开设置”默认进入账户；智能体页负责选择档位，并可直达模型服务配置每档模型。管理员自动看到“调试 → 保存对话请求”入口，无需右上角开关。
- **O4**（已确认）：平台配置由 **pointer-official 官网 API** 下发（`GET /api/llm/catalog` 等，见 §4.4）；不从百炼/DeepSeek 官网直接抓取。
- **O5**（已确认）：三档命名统一为「快速/标准/高级」（agent/media 原「专家」改为「高级」，computer 保持「高级」）。

## 8. 里程碑

- [x] M1 第 1 期方案评审通过（2026-08-08）
- [x] M2 设置页改造 + 返回导航（4.1）— commit `6bacd04c`
- [x] M3 主界面余额/充值（4.2）— commit `566afd3c`
- [x] M4 模型服务简化 + 三档 UI（4.3）— commits `ef0f2386` `9046bad1`
- [x] M4 补充：自定义服务删除即时持久化、Computer Primary 默认值统一、模型服务组件回归测试与维护文档同步（2026-08-14）
- [ ] M5 第 2 期：官网 `GET /api/llm/catalog` + pointer-app 目录化（4.4）
- [ ] M6 硬编码清理与回归验证

### 第 1 期备注
- AgentSettingsPanel 旧三档表格保留（高级调试入口），后续可再收敛（低优先级）。
- Web 端无 `getCloudPlatformMe` 等价接口，余额胶囊仅桌面 Tauri 平台模式显示（O1 工程化修正）。
