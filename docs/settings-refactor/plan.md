# 配置界面重构 — 实现方案

> 分支：`refactor/settings-ui`（独立 worktree：`/Users/starliu/pointer-all/pointer-app-settings`）
> 基线：`main @ 418d36f1`
> 状态：草案，逐步完善

## 1. 目标

1. **设置不再弹窗**：设置改为应用主界面的一等页面，通过「返回」回到主聊天界面。
2. **突出账户余额与充值入口**：主界面常驻可见余额，一键进入充值。
3. **LLM 配置简化**：只区分「平台」+「自定义」两类；每类仍分三档（快速/标准/高级）不变，优化每个档位的设置界面。
4. **平台配置不固化在代码里**：平台信息（服务地址、模型清单、默认参数、三档模型映射）从平台官网/后端加载，默认值由平台侧设置。

实现要求：分期实施（先界面、后逻辑），不降低风险，独立 worktree 开发。

---

## 2. 现状分析（基线调查结论）

### 2.1 设置弹窗

| 位置 | 说明 |
|---|---|
| `src/App.vue` | `showSettings` / `showSkills` 两个 ref 控制；`<SettingsDialog>` 以 fixed overlay 挂载在 `AppShell` 之上 |
| `src/components/settings/SettingsDialog.vue` | 960×740 弹窗：头部（技能/主题/调试开关/关闭 X）、左侧分区导航、右侧面板、底部「取消/保存」 |
| 分区 | `account` 平台账户 / `automation` 自动化 / `channels` 连接 / `assistant` 智能体 / 调试菜单（`provider` 模型服务、`generation` 界面配置、`agent` 智能模式）/ `cloud` 云主机（桌面·平台模式）/ `about` 关于 |
| `src/components/layout/AppShell.vue` | 主布局（侧栏+主区）；侧栏有「自动化/技能/连接」入口，通过 `emit('open-settings', section)` 打开弹窗并定位分区 |

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

---

## 3. 目标架构

```
主界面（AppShell）
 ├─ 左侧：会话/项目侧栏（不变）
 └─ 主区：视图切换
     ├─ chat 视图（默认）
     └─ settings 视图（新，一等页面，带返回按钮回到 chat）
          ├─ 分区导航（沿用现有分区，新增/调整）
          ├─ 账户（余额卡片 + 充值入口，主界面也有常驻入口）
          ├─ 模型服务（平台 + 自定义）
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
   - 头部返回按钮：`@back → mainView='chat'`；
   - Esc 键返回（设置页内未编辑时）；
   - 设置页内的「保存」成功后自动返回主界面（沿用现有 saveFromFooter 后再 emit('close') 的语义，改为 emit('back')）。
   - 有未保存编辑时返回：沿用现有 footer「保存」主路径，不额外加确认弹窗（保持现有交互强度）。
4. 调试菜单（provider/generation/agent 分区）保持可见性规则不变，只是从弹窗分区变为页面分区。
5. Skills 弹窗（`SkillPicker`）本次不改（非本需求范围），但入口联动逻辑保留。

**验收**：全屏/窗口下设置以页面形态呈现；打开设置不遮挡聊天；返回后聊天状态无损；深链分区定位正常；保存/取消行为与现状一致。

#### 4.2 主界面突出余额与充值

1. 数据：新增 `usePlatformBalance`（或并入 platformAuth store）：
   - 登录后调用 `getCloudPlatformMe()` 取 `balance_yuan`；
   - 会话刷新/余额耗尽事件后重取；低余额阈值（如 < 10 元）与耗尽态给不同样式。
2. UI：
   - 主界面头部（AppShell 顶栏右侧）常驻「余额」胶囊：`¥xx.xx` + 「充值」按钮（`openPlatformBillingPage()`）；
   - 未登录/standalone：不显示或显示登录入口（按平台模式判断）；
   - 余额耗尽时胶囊变警示色，点击直达充值页；聊天错误态「去充值」保持不变；
   - 设置页「账户」分区增加余额卡片（复用余额数据，避免各页面各自请求）。
3. 兼容：Web 端与桌面端都要可用；`getCloudPlatformMe` 在 web 是否有对应实现需确认（见开放问题 O1）。

**验收**：登录后主界面可见余额与充值入口；余额变化（购买/耗尽）能刷新；点击充值打开平台账单页。

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
4. 数据模型第 1 期不动：仍写回 `agentModeLlm` / `mediaModeLlm` / `computerTierLlm` / providers，保存链路沿用 `saveDebugSession`。
5. `AgentSettingsPanel.vue` 中的三档表格迁移/收敛到新「模型服务」页，避免两处维护；调试菜单里保留只读摘要或移除。

**验收**：非调试模式下普通用户也能配置模型服务；平台服务商只需密钥+三档；自定义服务商完整编辑不受影响；三档改动保存后运行时生效（用 `runTest` 或实际对话验证）。

### 第 2 期：配置逻辑改造（数据/后端）

**范围：移除平台配置硬编码，默认值平台化。**

#### 4.4 平台目录下发

1. 后端（平台侧 + `server/`）：
   - 新增平台目录数据源：`platformCatalog`（或并入 `EffectiveSettingsView.platform`）：
     ```ts
     interface PlatformCatalog {
       providers: PlatformProviderTemplate[]   // id/name/baseUrl/models
       modelMeta: Record<string, ModelMeta>    // capabilities 视觉/生图/生视频/语音
       tierDefaults: {                         // 三档默认映射
         agentModeLlm: ...; mediaModeLlm: ...; computerTierLlm: ...;
       }
       defaults: { activeProviderId, model, temperature, maxTokens, mediaModelOverrides }
     }
     ```
   - 平台模式由官方后端下发；standalone 模式由 `pointer-server.toml`（LLM providers）或内置最小目录兜底。
2. 前端：
   - 新增 `PlatformCatalogStore`（或在 settings store 内）：启动 `settings.load()` 时一并拉取目录；失败时用最小兜底（OpenAI 兼容 + 当前已有 providers），不阻塞启动。
   - 删除/降级硬编码：`providerParams.ts` 的 `PROVIDER_TEMPLATE_OPTIONS`、`settings.ts` 的 `defaultPlatformSettings` 模型默认值、`modelCapabilities.ts` 的模型清单与能力推断 —— 改为目录驱动；`isQwenProvider`/`isDeepSeekProvider` 启发式改为目录标记（`variant: 'qwen'|'deepseek'|'generic'`）。
   - 自定义服务商仍由用户创建（openai_compatible 变体），不受目录影响。
3. 默认值平台化：新建/重置时默认来自平台设置；「恢复默认」按钮调用平台默认值而非本地常量。
4. 兼容与迁移：
   - 旧用户已有 providers/三档覆盖保留（目录只补默认，不覆盖用户显式配置）；
   - 目录变更（模型上下架）不破坏已保存配置（缺失模型时提示并回退到同档默认）。

**验收**：修改平台目录（后端）后，前端无需发版即可看到新平台/模型/默认值；旧配置不回退、不丢失；无目录可用时应用仍可启动并提示。

---

## 5. 风险与兼容性

| 风险 | 对策 |
|---|---|
| 设置页改造影响启动性能 | 设置视图继续懒加载（`defineAsyncComponent`），首帧不加载 |
| 页面形态下保存/取消语义变化 | 保存成功自动返回；取消=返回且丢弃未保存编辑（与弹窗一致） |
| 余额接口在 web/standalone 不可用 | 按运行环境降级隐藏；错误静默，不阻塞 UI |
| 三档配置迁移到新页可能影响运行时 | 第 1 期保持数据字段与保存链路不变，仅改 UI 组织；迁移后用现有 `runTest`/对话验证 |
| 平台目录缺失/变更导致默认值漂移 | 目录加载失败用当前本地值兜底；目录变更不覆盖用户显式配置 |
| 删除硬编码可能破坏 standalone 模式 | 保留 `pointer-server.toml` 作为 standalone 的配置源，前端目录改为「平台模式=远程 / standalone=配置+兜底」 |

## 6. 测试策略

- 组件测试：`SettingsView` 视图切换、余额胶囊状态、三档编辑器组件（沿用 vitest 结构）。
- 手动验收清单（第 1 期）：打开设置→返回；深链分区；保存后返回；余额显示/充值；平台+自定义两类配置；三档修改后对话生效。
- 第 2 期：模拟目录接口返回不同数据验证渲染；旧配置迁移用例。

## 7. 开放问题（待确认）

- **O1**：余额数据源 `getCloudPlatformMe` 在 Web 端是否有对应实现？主界面余额胶囊的展示范围（仅桌面平台模式，还是 web 也要）？
- **O2**：设置作为页面后，左侧会话侧栏是否保留？建议保留（与聊天同屏），返回按钮只切主区。若希望全屏沉浸式设置，需另做。
- **O3**：「平台 + 自定义」中，平台服务商是否需要支持用户自建多实例（如两个千问账号）？建议第 1 期保持「每平台一个，密钥可换」，多实例归入自定义。
- **O4**：第 2 期「平台官网加载」指 Pointer 平台后端下发目录，还是直接从各模型官网（百炼/DeepSeek）抓取？方案按 Pointer 后端下发设计（可控、稳定），如需直接抓官网需单独评估。
- **O5**：三档命名统一为「快速/标准/高级」，是否接受（当前 agent/media 用「专家」、computer 用「高级」）？

## 8. 里程碑

- [ ] M1 第 1 期方案评审通过
- [ ] M2 设置页改造 + 返回导航（4.1）
- [ ] M3 主界面余额/充值（4.2）
- [ ] M4 模型服务简化 + 三档 UI（4.3）
- [ ] M5 第 2 期：平台目录接口 + 前端目录化（4.4）
- [ ] M6 硬编码清理与回归验证
