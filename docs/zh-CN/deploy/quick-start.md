# 四种部署模式的本地快速启动

[English](../../en/deploy/quick-start.md) | 简体中文

打包与上线步骤见 [README.md](README.md)。这一页只做一件事：在本机拉起一套和冒烟测试相同的进程，用来确认某一格部署模式能起来。

入口是仓库根目录的 `scripts/quick-deploy.mjs`（`npm run quick-deploy -- …`）。它不读取 `pointer.local.env`，每种模式自己设置环境变量，避免本机的 managed 配置漏进 standalone。状态、日志和生成的本地密码写在 `.quick-deploy/`（已 gitignore）。

两种启动方式用同一个入口，用 `--runtime` 区分：

| `--runtime` | 默认 | 启动什么 | 会不会编译 |
| --- | --- | --- | --- |
| `dev` | 是 | `cargo run`、Vite、`tauri dev` | 会，第一次可能要几分钟 |
| `bin` | 否 | `target/release/pointer-server` 或 `target/release/pointer-app` | 不会，缺二进制就直接退出 |

开发态的四种模式：

| 模式 | 做什么 | 起来之后怎么认 |
| --- | --- | --- |
| `standalone-server` | `cargo run -p pointer-server` + `npm run web:dev`，自动生成账密 | `GET /api/auth/mode` 为 `standalone`，浏览器打开 `http://127.0.0.1:1420/` |
| `standalone-client` | `npx tauri dev`，数据目录单独放在 `.quick-deploy/standalone-client/data` | Vite 在 `127.0.0.1:1420` 监听，桌面窗口出现 |
| `managed-server` | 同上，但 `POINTER_EDITION=managed` 且 `POINTER_DEPLOYMENT_MODE=platform` | `GET /api/auth/mode` 为 `platform` |
| `managed-client` | `tauri dev`，编译和运行都带上三个控制面域名 | 桌面进程以 managed 口味启动 |

开发态的服务端网页和桌面客户端都占用 Vite 端口 `1420`，同时只能跑一个。服务端 API 固定在 `127.0.0.1:8787`，和 Vite 的 `/api` 代理一致。发布版服务端自己托管 `dist/`，网页也在 `http://127.0.0.1:8787/`，不再占用 `1420`。发布版客户端同样不占用 `1420`。开发态和发布版的服务端不能同时听 `8787`。

## standalone 服务端

```bash
npm run quick-deploy -- start standalone-server
```

第一次启动会在 `.quick-deploy/standalone-server/standalone-admin.env` 写下用户名 `admin`、随机密码和 HMAC。终端会打印密码一次。再次启动复用这份文件，不再打印密码。

```bash
npm run quick-deploy -- status
npm run quick-deploy -- stop standalone-server
```

登录用账密加验证码。模型 Key 仍在设置里填写。

## standalone 客户端

```bash
npm run quick-deploy -- start standalone-client
```

不设置 `POINTER_EDITION`，也不绑定控制面。数据目录与已安装的 `PointerApp` 分开。

## managed 服务端

需要三个控制面域名。没有真实控制面时，进程仍会以 platform 模式起来，但浏览器登录要等控制面可访问。

```bash
npm run quick-deploy -- start managed-server \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

也可以导出 `POINTER_API_BASE`、`POINTER_WEB_BASE`、`COMPUTER_ANNOTATE_API_BASE`，再省略这三项参数。

## managed 客户端

域名和上面相同。`cargo` 在 `POINTER_EDITION=managed` 时会把域名编进二进制，缺任何一个都会让构建失败。

```bash
npm run quick-deploy -- start managed-client \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

## 编译后的二进制

先在本机打好对应产物。服务端是 `npm run server:build`（前端 `dist/` 和 `target/release/pointer-server`）。客户端是 `npm run tauri:build`（`target/release/pointer-app`）。脚本发现文件不在就退出，不会临时编译。

```bash
npm run quick-deploy -- start standalone-server --runtime bin
npm run quick-deploy -- start standalone-client --runtime bin
```

发布版 standalone 服务端的账密写在 `.quick-deploy/bin/standalone-server/standalone-admin.env`，和开发态那份分开。浏览器打开 `http://127.0.0.1:8787/`。`GET /api/health` 为 `ok`，`GET /api/auth/mode` 为 `standalone`。

managed 同样加上 `--runtime bin` 和三个域名。脚本只把域名放进进程环境后启动已有二进制，不会重新编译。若二进制本身是用 `POINTER_EDITION=managed` 编出来的，构建那一步仍然需要这三个域名。

```bash
npm run quick-deploy -- start managed-server --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com

npm run quick-deploy -- start managed-client --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

`stop standalone-server` 会停掉这个模式的开发态和发布版。只停发布版：

```bash
npm run quick-deploy -- stop standalone-server --runtime bin
```

## 只看计划、不启动

```bash
npm run quick-deploy -- plan standalone-server
npm run quick-deploy -- plan standalone-server --runtime bin
npm run quick-deploy -- plan managed-client --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

`stop` 不带模式名会停掉 `.quick-deploy/` 里记录的全部模式和两种 runtime。停止时按 pid 文件结束对应进程，不会按进程名去杀。
