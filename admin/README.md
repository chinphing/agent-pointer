# pointer-admin

企业控制面：本地账号、RBAC、统一模型密钥、技能/插件下发、用量与审计。
**独立进程**，`pointer-server` 与桌面客户端都是它的 OAuth 客户端。

设计文档：[docs/zh-CN/design/enterprise-admin-console.md](../docs/zh-CN/design/enterprise-admin-console.md)

## 跑起来

```bash
# 首次启动会自动建库、建表、建首个管理员（来自下面的 BOOTSTRAP_*）
POINTER_ADMIN_BOOTSTRAP_USER=admin \
POINTER_ADMIN_BOOTSTRAP_PASSWORD='改成强密码' \
POINTER_ADMIN_OAUTH_SECRET='随便一个长随机串' \
cargo run -p pointer-admin
```

管理端 UI：<http://127.0.0.1:8788>（前端产物在 `admin/web/dist`，没有就显示占位页）

## 环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `POINTER_ADMIN_ADDR` | `127.0.0.1:8788` | 监听地址 |
| `POINTER_ADMIN_DB_URL` | `sqlite:pointer-admin.db` | 支持 SQLite / PostgreSQL / MySQL |
| `POINTER_ADMIN_DATA_DIR` | `admin/data` | 技能/插件包存放目录 |
| `POINTER_ADMIN_STATIC_DIR` | `admin/web/dist` | 管理端前端产物 |
| `POINTER_ADMIN_BOOTSTRAP_USER` / `_PASSWORD` | 无 | 库为空时创建首个管理员 |
| `POINTER_ADMIN_OAUTH_SECRET` | 无 | 没配则 `/oauth/*` 与 `/auth/app/token` 返回 503 |
| `POINTER_ADMIN_MASTER_KEY` | 无 | **多实例必配**。不配则密钥按本机标识加密，换机解不开 |
| `POINTER_ADMIN_TENANT` | `default` | 多租户预留，目前单租户 |

## 测试

```bash
cargo test -p pointer-admin                      # 单测（SQLite 内存库）
bash admin/scripts/smoke.sh                      # 端到端冒烟（真起进程、真发 HTTP）

# 真机数据库（需要显式给一个专用测试库，否则跳过）
POINTER_ADMIN_TEST_DB_URL='postgres://127.0.0.1:5432/pointer_admin_test' \
  cargo test -p pointer-admin --test db_backends
POINTER_ADMIN_TEST_DB_URL='mysql://root@127.0.0.1:3307/pointer_admin_test' \
  cargo test -p pointer-admin --test db_backends
```

冒烟脚本支持 `PORT=` / `DB=` / `POINTER_ADMIN_DATA_DIR=` 覆盖，每次换一个新端口跑。

## 多实例部署

三条硬要求，缺一条就会出随机故障：

1. 同一个 PostgreSQL / MySQL（SQLite 只适合单机）
2. 同一个 `POINTER_ADMIN_MASTER_KEY`
3. 同一个 `POINTER_ADMIN_DATA_DIR`（共享卷）——包存在本地盘，不共享则只有上传的那台能下载

细节与自查清单见设计文档「13.4 部署形态」。

## 目录

```text
admin/
├── src/
│   ├── oauth/        OAuth 授权码/令牌/partner 接口
│   ├── http/         管理端 REST（账号、角色、资源、用量、审计）
│   ├── schema.rs     建表与迁移（v1…v5）
│   ├── crypto.rs     企业密钥加密（AES-256-GCM）
│   └── versions.rs   技能/插件包版本与下发
├── web/              管理端前端（Vue 3 + Vite，独立工程）
└── scripts/smoke.sh  端到端冒烟
```
