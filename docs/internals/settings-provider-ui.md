# 设置页：模型服务商编辑（维护说明）

> **可见性**：设置侧栏「模型服务」仅在 **平台管理员 + 调试模式** 下显示（`canEditPlatform && debugMenusEnabled`）。普通用户通过平台账户 OAuth 注入 API Key，不可编辑服务商列表。

> **关闭调试**：Bug 按钮再次点击会同步关闭 `rawContentViewEnabled`、`computerAnnotatedScreenViewEnabled`、`debugDumpLlmPrompts`、`taskBoardShowChildBoards`，并清除各智能体 `agentUiOverrides` 中的 `showSidecarToolCalls` / `showToolCallResults` / `showReasoning`（恢复 profile 默认）。

> 配置写入内存，重启后恢复默认；底部「保存(本次会话)」通过专用的
> `DebugSessionSettings` 边界更新当前进程配置。
>
> **WEB 回读**：平台管理员（含 standalone 本地管理员）的 `/api/settings` 响应会保留
> `agentModeLlm` / `debugMenusEnabled` 等调试字段，保存后再打开设置不会退回内置默认。
> 非管理员响应仍省略这些字段；前端在字段缺失时保留当前内存值。

## 调试会话配置边界

- 模型服务商、当前模型、生成参数及电脑/智能体/多媒体模型映射统一通过
  `DebugSessionSettings` 更新。
- APP 使用专用 Tauri command，WEB 使用
  `PUT /api/debug-session-settings`；两端最终调用同一个 Core 更新方法。
- Core 只原子替换 `AppState.platform_config` 中对应字段，不调用 storage、
  `PersistedLocalPlatformSettings` 或其他持久化写入路径。
- `PersistedLocalPlatformSettings` 必须继续排除 providers、当前服务商/模型、
  temperature、maxTokens 及所有调试模型映射。
- WEB 接口沿用平台访问鉴权与响应脱敏，不得在日志中记录 API Key。

## 保存快照规则

设置页必须在第一个异步请求之前同步构造所有请求的不可变快照。
后续主题或用户设置请求可能用服务端旧值刷新 Store，但不得据此重新构造
调试配置 payload。调试请求完成后再用返回的有效设置刷新 Store，确保新模型
立即用于后续对话。

## 模型能力标记

- 各模型在「定制 → 设置」弹窗顶部可勾选：**支持视觉理解**、**可生成图片**、**可生成视频**。
- 这些能力字段与温度等运行参数一样，属于有效定制覆盖；保存时不得因「仅改能力」被裁掉。
- 定制弹窗内的能力勾选必须通过父组件替换 `editingProvider.modelConfigs` 引用写入，
  禁止在子组件里直接改 props（Vue 只读代理下会丢改动）。
- `hasEffectiveModelOverride` / `pruneInheritedModelConfigs` / `sanitizeProviderModelConfigs`
  必须保留与默认值不同的 `supportsVision` / `canGenerateImage` / `canGenerateVideo`。
- 图片/视频生成下拉、vision 能力检测会读取 `modelConfigs` 中对应字段；未设置时对已知模型名自动推断。
- 千问 / 豆包默认列表已包含 Wan、Seedream、Seedance 等生成模型。

## 模式选择与调试模型映射

- 通用 / 编程 Agent、多媒体理解、电脑操控：用户在 **设置 → 智能体 → 模式选择** 中选运行模式；具体模型在调试模式下于 `agentModeLlm` / `mediaModeLlm` / `computerTierLlm` 配置（平台内存，重启恢复默认，相同持久化策略）。
- **API Key 回退**：主会话 / 子 Agent 按模式解析出的 Provider **没有可用 API Key**，但当前活跃 Provider 有 Key 时，自动回退到活跃 Provider；模型优先用原活跃模型，若不在该 Provider 的 `models` 列表中则改用列表首项（打 warn 日志）。有 Key 时仍优先用模式映射，不静默改道。

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

## 新增服务商与千问/深度求索一致

- 添加/编辑表单顶部有 **服务类型**（千问 / 深度求索 / OpenAI 兼容），决定 `RuntimeParamsForm` 的 variant（深度思考、推理力度等）。
- 类型可由 `id` / `baseUrl` 自动识别（如填 DashScope 地址会切到千问面板）；添加时也可直接点类型按钮套用默认 ID、地址与模型列表。
- 配置流程与内置服务商相同：服务商级 `RuntimeParamsForm` → 模型列表 → 各模型「同上 / 定制」→ 定制弹窗内同一套 `RuntimeParamsForm`。
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
- 仅点底部「保存配置」时，必须先 `flushEditingProviderToStore()`，否则会保存旧的 `providers`、新模型丢失。
- 单模型「设置」弹窗点「完成」：只 `closeModelConfigModal()`，**不要** `emit('close')`。
- 服务商表单「添加」/「保存」：写入成功后**退出**编辑区，回到上方服务商列表；继续改再点扳手。
- 底部「保存配置」：合并草稿后 `emit('close')` 关闭整个设置对话框。
- `applyProviderSnapshotToStore(..., reopenEdit)`：服务商表单添加/保存、底部保存前均用 `reopenEdit: false`。
- 新增服务商时若 **服务 ID 与已有重复**，`addProvider` 会拒绝并提示，避免 `find` 命中旧条目导致像没保存上。
