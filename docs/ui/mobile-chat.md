# 移动端聊天适配

窄屏（`<768px`）与触控（`hover: none`）下的聊天主区约定，App 客户端与 Web 共用。

## 消息区

- **角色头像**：用户 / 助手头像（`.message-avatar-slot`）在移动端与触控端**不显示**，避免列外绝对定位挤出边框与横向溢出。桌面宽屏仍为悬停显现。
- **时间戳与复制**：`.message-footer-actions` 默认收起；桌面悬停 / `:focus-within` 显现。移动端与触控端**不强制常显**（勿再写 `hover: none` 下 `opacity: 1`）。

## Composer

- 窄屏为**单行控件布局**：`[附件] [输入框] [发送]`（控件横排，不是固定单行高度）；宽屏仍为「输入在上、工具栏在下」。
- 输入框默认一行高，多行时**仅输入区向上增高**（上限约 250px）；附件 / 发送按钮底对齐，贴在输入区底部，不悬在中间。
- 宽屏 `md:flex-col` 下输入框必须 `md:flex-none`：若保留 `flex-1`（`flex-basis: 0%`），纵向主轴会**忽略** JS/`height`，表现为完全不撑高。窄屏横排仍用 `flex-1` 占满宽度。
- 优先 CSS `field-sizing: content`（`.composer-textarea`）；不支持时再 JS 测高。超过上限用 `overflow-y: auto`，勿用 `overflow-hidden` 以免超限内容无法滚动。
- 侧栏折叠 / 窗口变宽会改变换行：对 textarea 做 `ResizeObserver`，宽度变化时重测。
- MessageList 已对 scroller 高度变化 `ResizeObserver` 贴底（Composer 增高时列表视口变矮），无需额外接线。
- footer / welcome `inline` 共用同一 Composer；草稿清空、发送后、IME 结束、prefill 都会触发 `autoResize`。
- **智能体选择**与**项目选择**仅在 `md+` 工具栏显示；窄屏隐藏，沿用当前会话已选智能体 / 项目（或默认）。
- 默认提示文案可由服务端配置（`[server].composer_placeholder` / `POINTER_SERVER_COMPOSER_PLACEHOLDER`），写入 `index.html` meta `pointer-composer-placeholder`；前端 `resolveComposerPlaceholder()` 读取。登录态 / 余额 / Key 缺失时仍用固定提示覆盖。

## 实现位置

- `src/styles/globals.css` — 头像 / 页脚可见性、`.composer-shell` 内边距
- `src/components/chat/Composer.vue` — 窄屏单行布局
- `src/lib/webBranding.ts` — 提示文案解析
- `server/src/main.rs` — `apply_web_branding` 改写 title + meta
