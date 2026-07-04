# 桌面 OAuth 与官网首页提示

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
用户（桌面）→ 浏览器授权页 → 官网带 code 重定向到 127.0.0.1/callback
→ 桌面 loopback 收 code、302 → 首页 ?desktop_oauth=success → 官网 Toast → 去掉 query
```

桌面端换票在 loopback 收到请求后异步进行，与浏览器跳转并行，用户无需停留在 `127.0.0.1`。
