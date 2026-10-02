# 桌面 OAuth 与官网首页提示

[English](../../en/developer/desktop-oauth-web-integration.md) | 简体中文

桌面客户端完成 loopback 回调后，会向浏览器返回 **302**，跳转到官网首页并附带专用 query，由官网展示「登录成功」类提示。正常访问首页不会出现该提示。

## 跳转 URL

```
{POINTER_WEB_BASE}/?desktop_oauth=success
```

常量定义见 `crates/pointer-core/src/platform_auth.rs`：`DESKTOP_OAUTH_SUCCESS_QUERY`、`DESKTOP_OAUTH_SUCCESS_VALUE`。

## 官网实现（pointer-official）

已实现：`pointer-official/apps/web/src/components/DesktopOauthSuccessToast.tsx`，并在 `app/page.tsx` 挂载。

- 仅当 `desktop_oauth=success` 时展示 `SimpleToast`，随后 `history.replaceState` 去掉 query。
- 正常访问首页无该参数，不展示提示。

## 流程

```text
用户（桌面）→ bind 127.0.0.1 + 本机回环自检 → 打开浏览器授权页
→ 官网带 code 重定向到 127.0.0.1/callback
→ 桌面 accept 循环直到合法 code（非法/空连接快速忽略）→ 302 到首页 ?desktop_oauth=success
→ 换票期间 listener 保持：再次访问同样 302 到首页（避免停在 127.0.0.1）
→ 换票结束关闭 listener；客户端在 exchange 成功后立即结束「等待授权」
```

每次登录会轮换 `127.0.0.1` 端口（19427 起扫描），避免浏览器 keep-alive 复用导致「第一次成功、退出后再登卡住」。

关键日志前缀：`platform_auth: bound port` / `loopback probe` / `accepted oauth callback` / `exchange start|done` / `listener closed`。
