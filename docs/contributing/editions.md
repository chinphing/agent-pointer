# 构建版本（community / official）

同一份 `main`，两次构建。不要开长期 `commercial` 分支再往回合并。

## `POINTER_EDITION`

| 值 | 默认域名 | 用量上报 | standalone License | Tauri 更新 |
| --- | --- | --- | --- | --- |
| 未设置 | 与今天本地开发相同（生产域名，除非 standalone） | 非 standalone 为开 | 独立部署要校验 | 按 `tauri.conf.json` |
| `community` | 空，需自己设 `POINTER_*` | 默认关 | 不强制 | `tauri.community.conf.json` 关闭产物与端点 |
| `official` | readflowai.com | 非 standalone 为开 | 独立部署要校验 | 官方公钥与更新地址 |

运行时仍可用环境变量覆盖：`POINTER_API_BASE`、`POINTER_WEB_BASE`、`COMPUTER_ANNOTATE_API_BASE`、`POINTER_USAGE_REPORT_ENABLED`。前端对应 `VITE_POINTER_EDITION`、`VITE_POINTER_WEB_BASE`。

## 怎么打

社区包：

```bash
POINTER_EDITION=community VITE_POINTER_EDITION=community npm run tauri:build
```

官方包由 [`.github/workflows/release.yml`](../../.github/workflows/release.yml) 注入 `POINTER_EDITION=official` 和签名 secret。公钥可以留在仓库；私钥和 Apple 证书只放 Actions。

## 日常开发

未设置 edition 时，`npm run tauri:dev` 保持现有联调行为。不要把生产域名或签名口令写进公开 `main`。

## 维护流程

- 通用功能：公开仓库 PR → 社区构建和官方构建都会带上
- 只对付费云能力：改私有云 / 计费 / 更新服务
- 换域名、更新公钥、证书：只改官方 CI
- 外部 PR 默认进社区构建；合入前确认没有写死官方云

## 推到 GitHub 前仍需人工做的事

1. 轮换曾经进入 Git 历史的 Apple 证书口令；若仓库已被他人克隆，重签 Developer ID
2. 清洗后的副本在仓库旁的 `agent-pointer`（已去掉 `signing.env` 和打码服务密钥）。不要 force-push Codeup
3. 在 GitHub 打开 Private vulnerability reporting，并把官方签名 secret 只放进 Actions environment
