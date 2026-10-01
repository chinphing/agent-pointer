# 调试会话设置设计

## 目标

调试模式下修改的模型服务、模型列表和模型映射只保存在当前进程内存中，
保存后立即影响后续对话，应用重启后恢复平台默认值。

调试参数不得写入 `local_platform_settings.json`、`user_settings.json`
或其他持久化文件。

## 根因

设置页当前同时维护表单状态、`settings`、`platformSettings` 和 Rust
内存平台配置。底部保存先执行 `saveUser`，其返回值会刷新整个 Store；
调试 payload 随后才从 Store 构造，因此用户刚选的模型已被旧值覆盖。

现有保存接口还混合了持久化设置与会话调试设置，使保存顺序变化容易再次
造成覆盖。

## 方案

新增独立的 `DebugSessionSettings` DTO 和
`update_debug_session_settings` 接口：

- APP：Tauri command。
- WEB：`PUT /api/debug-session-settings`。
- Core：仅原子更新 `AppState.platform_config` 的调试字段。
- 不调用任何 storage/persist 函数。
- 返回完整 `EffectiveSettingsView`，供前端统一刷新。

DTO 包含：

- `providers`
- `activeProviderId`
- `model`
- `temperature`
- `maxTokens`
- `computerTierLlm`
- `computerPipelineLlm`
- `agentModeLlm`
- `mediaModeLlm`

前端点击保存时先生成不可变快照，再执行任何异步请求。调试页保存只调用
新的会话接口；主题等持久化设置可单独保存，但不得再参与调试 payload
的构造。

## 兼容性

- APP 与 WEB 使用同一 DTO 和 Core 更新函数。
- WEB 仍不开放通用平台设置写入；只允许已授权用户更新当前服务进程内存。
- macOS、Windows、Linux 均不依赖平台文件路径或专有 API。

## 可观测性

- 成功更新输出 info 日志，包含 provider 数量和映射数量，不记录 API Key。
- 权限拒绝和请求失败返回明确错误，不静默回退默认值。

## 测试

- Core：更新后有效设置包含非默认模型。
- Core：调试更新函数不触发本地持久化路径。
- 前端：主题保存返回旧设置时，已快照的调试 payload 仍保留新模型。
- APP/WEB transport：均调用独立接口。
- 构建与现有设置测试通过。
