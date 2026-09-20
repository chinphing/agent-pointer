# Web branding：欢迎提示与回合耗时前缀

Standalone server 可覆盖空会话欢迎提示与「工作」耗时前缀，路径与 `page_title` / `composer_placeholder` 相同：`[server]` → env → `index.html` meta → 前端 `webBranding.ts`。

独立部署配置总表与 TOML 示例见 [`../developer/standalone-deployment.md`](../developer/standalone-deployment.md)、[`../user/standalone-server.md`](../user/standalone-server.md)、[`../../server/pointer-server.toml.example`](../../server/pointer-server.toml.example)。

**产品默认文案不变**：未配置时仍是 slogan 欢迎页与 `工作 N m SS s` / `工作耗时未知`。

## 配置

| TOML `[server]` | Env | Meta `name` | 默认 |
|-----------------|-----|-------------|------|
| `welcome_tip_title` | `POINTER_SERVER_WELCOME_TIP_TITLE` | `pointer-welcome-tip-title` | 无 tip |
| `welcome_tip_body` | `POINTER_SERVER_WELCOME_TIP_BODY` | `pointer-welcome-tip-body` | 无 tip |
| `turn_elapsed_active` | `POINTER_SERVER_TURN_ELAPSED_ACTIVE` | `pointer-turn-elapsed-active` | `工作` |
| `turn_elapsed_done` | `POINTER_SERVER_TURN_ELAPSED_DONE` | `pointer-turn-elapsed-done` | `工作` |
| `brand_name` | `POINTER_SERVER_BRAND_NAME` | `pointer-brand-name` | `Pointer` |
| `brand_icon` | `POINTER_SERVER_BRAND_ICON` | `pointer-brand-icon` | `/app-icon.png` |
| `desktop_snapshot_enabled` | `POINTER_SERVER_DESKTOP_SNAPSHOT_ENABLED` | `pointer-desktop-snapshot` (`1`/`0`) | 自动探测显示器；无桌面则隐藏截图按钮 |

可选字段仅在非空时写入 meta。本地 Vite 可用同名 `VITE_*` 覆盖（优先于 meta）。

示例：

```toml
[server]
page_title = "财务报销助手"
composer_placeholder = "上传发票或说明…"
welcome_tip_title = "我是财务报销助手"
welcome_tip_body = "您提交附件后我会自动帮你填报销单，预计 10–30 分钟，期间您可以离开，完成任务后您回来确认信息即可。"
turn_elapsed_active = "报销单填写中"
turn_elapsed_done = "报销单已填写"
brand_name = "财务助手"
brand_icon = "/branding/logo.png"
desktop_snapshot_enabled = false
```

## 行为约定

1. **欢迎 tip**：仅在**全新空会话**（`shouldShowWelcomeHome`：内存与持久化消息数均为 0）展示；有消息或 hydrate 失败空窗不展示。桌面有 tip 时替换默认 slogan；移动端在欢迎区顶部展示（footer composer 不变）。
2. **耗时前缀**：进行中用 active；结束后用 done。未知耗时为 `{prefix}耗时未知`（默认仍为 `工作耗时未知`）。
3. **默认收缩执行过程**：不由 server 强制。定制部署若要在进行中看到收起条，请在助手设置打开「默认收缩执行过程」（见 [turn-elapsed.md](turn-elapsed.md)）。
4. **品牌名 / 图标**：侧栏/顶栏品牌字与左上角、左下角 logo 读取 `brand_name` / `brand_icon`（两处共用同一图标）；未配置时保持产品默认 `Pointer` 与 `/app-icon.png`。
5. **桌面截图按钮**：无显示器（headless / 非 UI）默认不显示；三个入口（桌面 / Web / standalone）同一套 meta。可强制 `desktop_snapshot_enabled = true|false`。
6. **非管理员**：不能打开设置（账户菜单隐藏「设置」、相关入口与 `openSettings` 均拦截）；右侧工作区按钮与面板不显示。

## 不做

- 改产品默认「工作」文案
- Server 强制 `collapseProcessByDefault`
- 交付 Markdown 表格样式定制（沿用 `.md-body`）

## 实现位置

- `crates/pointer-core/src/server_config.rs`
- `server/src/main.rs` — `apply_web_branding`
- `src/lib/webBranding.ts` / `turnElapsed.ts`
- `src/components/chat/WelcomeTipBanner.vue` / `ChatView.vue`
- 交互稿：[`../design/mobile-reimburse-assistant-mockup.html`](../design/mobile-reimburse-assistant-mockup.html)
