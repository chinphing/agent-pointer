# 设置页：模型服务商编辑（维护说明）

> **可见性**：设置侧栏「模型服务」仅在 **平台管理员 + 调试模式** 下显示（`canEditPlatform && debugMenusEnabled`）。普通用户通过平台账户 OAuth 注入 API Key，不可编辑服务商列表。

> **关闭调试**：Bug 按钮再次点击会同步关闭 `rawContentViewEnabled`、`computerAnnotatedScreenViewEnabled`、`debugDumpLlmPrompts`、`taskBoardShowChildBoards`，并清除各智能体 `agentUiOverrides` 中的 `showSidecarToolCalls` / `showToolCallResults` / `showReasoning`（恢复 profile 默认）。

> 配置写入内存，重启后恢复默认；底部「保存(本次会话)」调用 `saveModelService`。

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
- **定制**：`hasEffectiveModelOverride` 为 true 的条目才会写入磁盘。
- 加载设置时 Rust **不会**再为每个模型自动填充 `model_configs`（否则 reload 后全部变成定制）。
- `startEditProvider` 会 `pruneInheritedModelConfigs`；`buildProviderSnapshotFromEditor` 保存前同样按有效覆盖过滤。

## 新增模型后无法保存

- 模型名写在「模型列表」输入框（`editingModelsText`），须通过 `buildProviderSnapshotFromEditor` 合并进 `snapshot.models` 再 `updateProvider`。
- 仅点底部「保存配置」时，必须先 `flushEditingProviderToStore()`，否则会保存旧的 `providers`、新模型丢失。
- 单模型「设置」弹窗点「完成」：只 `closeModelConfigModal()`，**不要** `emit('close')`。
- 服务商表单「保存/添加」：写入磁盘后退出编辑区（`reopenEdit: false`），回到服务商列表；**不要**关闭整个设置对话框。
- 底部「保存配置」：合并草稿后 `emit('close')` 关闭整个设置对话框。
- `applyProviderSnapshotToStore(..., reopenEdit)`：底部保存前用 `reopenEdit: false`；服务商表单保存用 `true`。
- 新增服务商时若 **服务 ID 与已有重复**，`addProvider` 会拒绝并提示，避免 `find` 命中旧条目导致像没保存上。
