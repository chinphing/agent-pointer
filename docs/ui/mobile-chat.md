# 移动端聊天适配

窄屏（`<768px`）与触控（`hover: none`）下的聊天主区约定，App 客户端与 Web 共用。

## 消息区

- **角色头像**：用户 / 助手头像（`.message-avatar-slot`）在移动端与触控端**不显示**，避免列外绝对定位挤出边框与横向溢出。桌面宽屏仍为悬停显现。
- **时间戳与复制**：`.message-footer-actions` 默认收起；桌面悬停 / `:focus-within` 显现。移动端与触控端**不强制常显**（勿再写 `hover: none` 下 `opacity: 1`）。

## Composer

- 窄屏为**单行控件布局**：`[附件] [输入框] [发送]`（控件横排，不是固定单行高度）；宽屏仍为「输入在上、工具栏在下」。
- 输入框默认一行高，多行时**仅输入区向上增高**（上限约 250px）；附件 / 发送按钮底对齐，贴在输入区底部，不悬在中间。
- 测量时用 `height: 0` 再读 `scrollHeight`，避免窄屏 flex 横排下 `height: auto` 量不到换行高度。
- **智能体选择**与**项目选择**仅在 `md+` 工具栏显示；窄屏隐藏，沿用当前会话已选智能体 / 项目（或默认）。
- 默认提示文案可由服务端配置（`[server].composer_placeholder` / `POINTER_SERVER_COMPOSER_PLACEHOLDER`），写入 `index.html` meta `pointer-composer-placeholder`；前端 `resolveComposerPlaceholder()` 读取。登录态 / 余额 / Key 缺失时仍用固定提示覆盖。

## 实现位置

- `src/styles/globals.css` — 头像 / 页脚可见性、`.composer-shell` 内边距
- `src/components/chat/Composer.vue` — 窄屏单行布局
- `src/lib/webBranding.ts` — 提示文案解析
- `server/src/main.rs` — `apply_web_branding` 改写 title + meta
