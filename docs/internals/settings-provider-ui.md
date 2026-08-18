# 设置页：模型服务商编辑（维护说明）

> **默认分区**：普通入口打开设置落在「智能体」。指定分区的入口（如自动化、输入框档位）仍直达对应栏。

> **可见性**：设置侧栏「模型配置」是常规分区，平台服务（千问、DeepSeek、豆包）**由平台目录下发**，客户端本地不再内置任何平台服务商/模型清单/档位默认；登录平台后按 `platformProviders` 模板自动创建，未登录时「平台服务」区为空。自定义服务包含其余 OpenAI 兼容服务。普通用户只读；平台管理员可编辑。平台服务由平台统一管理，**不显示删除入口**；仅自定义服务可删除。平台服务与自定义服务用同一套卡片（名称、地址、模型数）；平台密钥不展示，条目不可编辑/删除，可设为默认。

> **保存语义**：模型服务、当前服务/模型、生成参数和三档模型映射均通过 `updateUserSettings` 保存到 `user_settings.json`；Provider API Key 以 `enc:v1:` 加密落盘。删除自定义服务前必须明确确认；确认后立即保存，失败时前端恢复删除前的服务列表、默认服务和模型。

> **调试权限**：调试分区仅对平台管理员或 standalone 本地管理员显示。它包含「保存每轮对话请求」「原始内容查看」「标记截图查看」；普通用户不可见也不可修改后两项。

> **WEB 回读**：平台管理员（含 standalone 本地管理员）的 `/api/settings` 响应会保留
> `debugMenusEnabled` 等调试字段。三档映射（`agentModeLlm` / `mediaModeLlm` /
> `computerTierLlm`）是普通用户偏好，**非管理员 GET/PUT 也必须回传**，否则改标准档
> 会把已自定义的快速档冲回平台默认。
> 其余调试字段非管理员响应仍省略；后端保存时保留平台调试字段。

## 持久化边界

- 模型服务商、当前模型、生成参数及电脑/智能体/多媒体模型映射统一通过
  `updateUserSettings` 更新；APP 使用 Tauri command，WEB 使用
  `PUT /api/user-settings`，两端最终调用同一个 Core 用户设置持久化方法。
- `DebugSessionSettings` / `PUT /api/debug-session-settings` 仅保留为旧 API 兼容与测试边界；当前设置页面不调用它，不能把它作为模型服务保存链路。
- 保存模型服务时，当前编辑服务保留显式输入的 API Key；其他服务的 key 置空，由后端内存 key 池回填。`source=platform` 的平台注入服务不会写入用户层。
- 自定义服务删除后立即提交完整服务列表；保存失败必须恢复删除前的列表、默认服务和模型。
- WEB 接口沿用平台访问鉴权与响应脱敏，不得在日志中记录 API Key。

## 保存快照规则

设置页在第一个异步请求之前同步构造不可变快照。服务端返回有效设置后同步回填 Store，避免旧响应覆盖正在编辑的模型服务 key 或场景模型映射。

`saveUserSnapshot` 不能只回填 providers / 当前模型。场景档位（`agentPerformanceModes` / `mediaUnderstandingModes` / `computerInitialTier`）和三档映射也要写回 merged，否则输入框和下一轮对话仍读旧值。改档位时先写入内存，再异步落盘。

## 模型能力标记

- 各模型在「定制 → 设置」弹窗顶部可勾选：**支持视觉理解**、**可生成图片**、**可生成视频**。
- 这些能力字段与温度等运行参数一样，属于有效定制覆盖；保存时不得因「仅改能力」被裁掉。
- 定制弹窗内的能力勾选必须通过父组件替换 `editingProvider.modelConfigs` 引用写入，
  禁止在子组件里直接改 props（Vue 只读代理下会丢改动）。
- `hasEffectiveModelOverride` / `pruneInheritedModelConfigs` / `sanitizeProviderModelConfigs`
  必须保留与默认值不同的 `supportsVision` / `canGenerateImage` / `canGenerateVideo`。
- 图片/视频生成下拉、vision 能力检测会读取 `modelConfigs` 中对应字段。
  平台模型以目录下发的 `supportsVision` / `canGenerateImage` / `canGenerateVideo` /
  服务商 `reasoningInMessages` 为准；未下发时才按 **API 地址** 推断（DashScope → 视觉，DeepSeek API → 无视觉），不按服务商 id。
- 千问 / 豆包模型清单（含 Wan、Seedream、Seedance 等生成模型）**由平台目录下发**，本地不再内置默认模型列表。平台新增模型后，用户下次登录或刷新凭据即可在下拉中看到，无需发客户端版本。
- 场景档位「已覆盖」以平台 `tierDefaults` 为准，不要在界面里写死模型名来判断是否默认。

## 模式选择与调试模型映射

- 通用 / 编程 Agent、多媒体理解、电脑操控：用户在 **设置 → 模型配置** 中调整场景档位；`agentModeLlm` / `mediaModeLlm` / `computerTierLlm` 作为用户覆盖通过 `updateUserSettings` 持久化，重启后保留。用户层服务商只按 `source` 分层：`source=platform` 不落盘、读盘删除；不得按服务商 id 认平台。档位映射引用平台 id 仍是用户覆盖。
- **电脑操控档位 / Verify**：调试下拉使用全部已配置服务商的 `allModels`（值为 `providerId:model`），写入 `providerId` + `model`；运行时 `apply_round_settings` / `apply_pipeline_phase_settings` 会同时切换 `activeProviderId` 与 `model`。
- **API Key 回退**：主会话 / 子 Agent 按模式解析出的 Provider **没有可用 API Key**，但当前活跃 Provider 有 Key 时，自动回退到活跃 Provider；模型优先用原活跃模型，若不在该 Provider 的 `models` 列表中则改用列表首项（打 warn 日志）。有 Key 时仍优先用模式映射，不静默改道。

## 思考强度协议

- 产品界面只暴露「思考强度」（`off|low|medium|high|max` / 不设置）。
- 不在设置里展示思考协议、深度思考开关、思考预算或厂商力度。
- 线路字段由客户端按服务商自动翻译：千问→预算，DeepSeek→`reasoning_effort`，
  OpenRouter→`reasoning`，智谱/自定义 OpenAI 兼容→顶层 `reasoning_effort`。
  平台目录只下发强度，不下发协议。
- 场景三档映射同样只选强度。
- Agent 模式映射写入的思考参数会在 `apply_agent_model_defaults` 时落到本轮
  `round_*` 覆盖（含 `round_thinking_intensity`），再由策略翻译到请求体。

## 保存后界面「空白」

编辑区由 `v-if="editingProvider"` 控制。`saveProvider` 成功后**不要**把 `editingProvider` 设为 `null`，否则编辑表单消失，用户会以为配置页坏了。应使用 store 中规范化后的条目调用 `startEditProvider` 重新打开。

## Pinia 更新服务商

- **不要** `Object.assign(provider, patch)` 就地合并，尤其含 `modelConfigs` 时列表可能不刷新。
- **要** `normalizeProvider` 后替换 `providers[i]`，并 `settings.providers = [...list]`。

## `modelConfigs` 与渲染死循环

- 模板里**不要**在渲染时 `ensure` / 自动创建 `modelConfigs` 条目（会无限重渲染）。
- 写入时先 `cloneModelConfigs`，再整体替换 `editingProvider.modelConfigs` 对象。

## 数据规范化

- `normalizeProvider` 保证 `models` 为数组；`save()` 返回后须 `normalizeProviders`。
- 模板访问 `p.models` 使用可选链，避免旧数据缺字段导致整页报错。

## 参数表单复用

- `RuntimeParamsForm` + `useRuntimeParams`：服务商级用 `providerScopeModelId === null`，模型定制用 `modelConfigModalId`。
- 改 `modelConfigs` 时替换顶层对象引用（见 `useRuntimeParams.patchModel`）。
- **扩展参数 `extraBody`**：JSON 对象；失焦时解析写入。服务商级与模型级均可配；模型覆盖浅合并服务商。
  请求时一律展平到 chat/completions 根级（对齐 Hermes）。详见 [`../llm/model-thinking-api.md`](../llm/model-thinking-api.md)。

## 新增自定义服务

- 添加服务只列出自定义协议模板（OpenAI 兼容 / OpenRouter / Kimi / 智谱）。若某模板的默认 id 已出现在当前 `source=platform` 列表中，添加列表不再给出该项（避免自建一份平台目录已有的服务）。
- 编辑已有服务时，服务类型可由 `id` / `baseUrl` 自动识别（如填 DashScope 地址会切到千问协议面板）；平台注入服务（`source=platform`）只读，不可编辑删除。
- 配置流程与自定义服务相同：服务商级 `RuntimeParamsForm` → 模型列表 → 各模型「同上 / 定制」→ 定制弹窗内同一套 `RuntimeParamsForm`。
- 预设与识别逻辑在 `src/lib/providerParams.ts`（`PROVIDER_TEMPLATE_OPTIONS`、`detectProviderTemplateId`）。

## 模型「同上」与 `modelConfigs`

- **同上**：该模型在 `modelConfigs` 中**无条目**（或仅有与服务商默认相同的冗余字段，保存时会被剔除）。
- **定制**：`hasEffectiveModelOverride` 为 true 的条目才会写入当前进程配置
  （含与默认不同的视觉/生图/生视频能力）。
- 加载设置时 Rust **不会**再为每个模型自动填充 `model_configs`（否则 reload 后全部变成定制）。
- `startEditProvider` 会 `pruneInheritedModelConfigs`；`buildProviderSnapshotFromEditor` 保存前同样按有效覆盖过滤，且必须拷贝能力字段。

## 新增模型后无法保存

- 模型名写在「模型列表」输入框（`editingModelsText`），须通过 `buildProviderSnapshotFromEditor` 合并进 `snapshot.models` 再 `updateProvider`。
- **改模型名**：`updateProvider` 在更新的是**当前激活**服务商、且 `settings.model` 已不在新列表中时，必须改选 `models[0]`。否则后端校验 `active model is not configured for provider`，前端只显示「应用配置失败，请重试」。
- 单模型「设置」弹窗点「完成」：只 `closeModelConfigModal()`，**不要**关闭整个设置页。
- 服务商表单「添加」/「保存」：写入成功后退出编辑区，回到服务商列表；继续修改时再点扳手。
- 当前没有底部「保存配置」栏：服务商表单点击「添加」/「保存」时立即持久化；自定义服务点删除时也立即持久化。
- `applyProviderSnapshotToStore(..., reopenEdit)`：服务商表单添加/保存使用 `reopenEdit: false`。
- 新增服务商时若 **服务 ID 与已有重复**，`addProvider` 会拒绝并提示，避免 `find` 命中旧条目导致像没保存上。
