# Pointer 控制面前端

独立 Vite + Vue 3 工程，产物由 `pointer-admin` 进程静态托管。

## 开发

```bash
# 1. 起后端（另开一个终端，在仓库根目录）
POINTER_ADMIN_BOOTSTRAP_USER=admin \
POINTER_ADMIN_BOOTSTRAP_PASSWORD=admin12345 \
POINTER_ADMIN_OAUTH_SECRET=dev-secret \
cargo run -p pointer-admin

# 2. 起前端
cd admin/web
npm install
npm run dev          # http://127.0.0.1:5173，/api /oauth /auth 已代理到 :8788
```

## 构建（给控制面进程用）

```bash
cd admin/web && npm install && npm run build
```

产物落在 `admin/web/dist`。`pointer-admin` 启动时若检测到该目录会自动挂载：
`/` 返回 `dist/index.html`，`/assets/*` 由 `ServeDir` 提供。

## 约定

- **hash 路由**（`#/members`）：控制面是单进程静态托管，刷新任意页面都不会 404。
- **同源请求**：接口一律 `/api/admin/*`，靠 cookie 会话，不需要配 base URL。
- **权限在服务端判定**：界面用 `can('user:write')` 只是隐藏按钮，真正的拦截在 Rust 侧。
- 只做中文，不接 i18n。

## 页面

| 路由 | 内容 |
| --- | --- |
| `#/overview` | 账号数、资源分布、本月按模型用量 |
| `#/members` | 成员列表、新建、改角色、停用、重置密码、CSV 导入 |
| `#/roles` | 内置角色与权限矩阵（只读） |
| `#/resources` | 六类资源 CRUD、授权（全员/角色/成员）、查看明文密钥 |
| `#/usage` | 按成员/模型/日期聚合，配额设置 |
| `#/audit` | 审计流水，含改前改后 |
