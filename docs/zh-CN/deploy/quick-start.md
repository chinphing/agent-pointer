# 本地启动

[English](../../en/deploy/quick-start.md) | 简体中文

开发者和用户的启动方式不同。

## 现在就启动

用户用脚本直接启动桌面程序。脚本不调用 npm。不写 `--runtime` 时就是 `--runtime bin`。

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin
```

```bat
scripts\quick-deploy.bat start standalone-client --runtime bin
```

仓库里已经有的 `target/release/pointer-app` 会直接打开。没有这份文件时，用本机已经安装的 `pointer-app`。两处都没有时脚本退出。要在本地重新编译，自己加上 `--build`：

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin --build
```

`--build` 编完后启动的是刚生成的 `target/release/pointer-app`，不再改用已安装的程序。数据在 `.quick-deploy/bin/standalone-client/data`，和已经安装的 Pointer 分开。

浏览器访问已编译的服务端：

```bash
./scripts/quick-deploy.sh start standalone-server --runtime bin
```

页面在 `http://127.0.0.1:8787/`。第一次启动把账密写到 `.quick-deploy/bin/standalone-server/standalone-admin.env`，终端打印一次密码。再次启动复用这份文件，不再打印密码。自建服务端需要本机已经有 `pointer-server` 和 `dist/`。缺了就退出。加上 `--build` 才会本地编译（`node scripts/build-server.mjs`），然后再启动。

## 开发者改代码

开发者用 npm 启动开发态，会按改动编译：

```bash
npm install
npm run tauri:dev
```

`npm run tauri:dev` 读取 `pointer.local.env`。没有这个文件时不绑定控制面。第一次编译要几分钟。

在浏览器里改同一套界面，开两个终端：

```bash
npm run server:dev
npm run web:dev
```

打开 `http://127.0.0.1:1420/`。环境、调试和失败时要装的库见 [DEVELOPMENT.md](../../../DEVELOPMENT.md)。

## 再选别的格子

打包与上线步骤见 [README.md](README.md)。下面是同一套脚本的其余格子。脚本不读取 `pointer.local.env`，每种模式自己设置环境变量，避免本机的 managed 配置漏进 standalone。状态、日志和生成的本地密码写在 `.quick-deploy/`（已 gitignore）。Linux 和 macOS 用 `./scripts/quick-deploy.sh`，Windows 用 `scripts\quick-deploy.bat`。二进制的状态在 `.quick-deploy/bin/<模式>/`，开发态在 `.quick-deploy/<模式>/`，两套账密和 pid 分开。

| `--runtime` | 默认 | 启动什么 | 会不会重新编译 |
| --- | --- | --- | --- |
| `bin` | 是 | 桌面客户端：已有的 `target/release/pointer-app`，否则已安装的 `pointer-app`。服务端：已有的 `pointer-server` | 不编译。客户端两处都没有，或服务端缺二进制 / `dist/`，就退出。只有写了 `--build` 才本地重新编译 |
| `dev` | 否 | `cargo run`、本机 Vite、本机 Tauri CLI | 源码、`POINTER_EDITION` 或三个域名变了才重编。都没变时 Cargo 复用已有 debug 产物 |

开发者日常改代码用上面的 npm，不用脚本的 `--runtime dev`。`--runtime dev` 只在要用脚本把四种格子拉成开发态时使用。

| 模式 | `--runtime bin` 起来之后 | `--runtime dev` 起来之后 |
| --- | --- | --- |
| `standalone-server` | 页面在 `http://127.0.0.1:8787/`，`GET /api/auth/mode` 为 `standalone` | Vite 页面在 `http://127.0.0.1:1420/`，API 仍在 `8787` |
| `standalone-client` | 桌面窗口是 `pointer-app`，不占用 `1420` | Vite 在 `127.0.0.1:1420` 监听，桌面窗口出现 |
| `managed-server` | 同上，`GET /api/auth/mode` 为 `platform` | 同上，并且 `POINTER_EDITION=managed` |
| `managed-client` | 桌面进程带上三个控制面域名，不占用 `1420` | `tauri dev`，编译和运行都带上三个域名 |

开发态的服务端网页和桌面客户端都占用 Vite 端口 `1420`，同时只能跑一个。服务端 API 固定在 `127.0.0.1:8787`。发布版服务端自己托管 `dist/`，网页也在 `http://127.0.0.1:8787/`。开发态和发布版的服务端不能同时听 `8787`。

## standalone 服务端

```bash
./scripts/quick-deploy.sh start standalone-server --runtime bin
```

```bat
scripts\quick-deploy.bat start standalone-server --runtime bin
```

登录用账密加验证码。模型 Key 仍在设置里填写。

```bash
./scripts/quick-deploy.sh status
./scripts/quick-deploy.sh stop standalone-server --runtime bin
```

## standalone 客户端

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin
```

本地 standalone 构建不设置 `POINTER_EDITION`。`--build` 编 standalone 时会锁住口味和域名，`pointer.local.env` 填不进去。本机已安装的官网签名包仍由平台下发模型。

## managed 服务端

需要三个控制面域名。没有真实控制面时，进程仍会以 platform 模式起来，但浏览器登录要等控制面可访问。

```bash
./scripts/quick-deploy.sh start managed-server --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

```bat
scripts\quick-deploy.bat start managed-server --runtime bin --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

也可以导出 `POINTER_API_BASE`、`POINTER_WEB_BASE`、`COMPUTER_ANNOTATE_API_BASE`，再省略这三项参数。不写 `--build` 时，脚本只把域名放进进程环境后启动已有二进制。加上 `--build` 才按这次传入的域名本地重新编译。构建缺一个域名就会失败。开发者打这个包也可以用 `npm run server:build`。

`--runtime dev` 才会执行 `cargo run -p pointer-server` 和本机 Vite。Cargo 只在源码、`POINTER_EDITION` 或三个域名变化时重编。

## managed 客户端

```bash
./scripts/quick-deploy.sh start managed-client --runtime bin \
  --api-base https://pointer-api.example.com \
  --web-base https://pointer.example.com \
  --annotate-base https://pointer-som.example.com
```

不写 `--build` 时用已有的 `target/release/pointer-app` 或已安装的程序。加上 `--build` 才按这次传入的三个域名本地重新编译。开发者打这个客户端也可以用 `npm run tauri:build`。

## 只看计划、不启动

```bash
./scripts/quick-deploy.sh plan standalone-server
./scripts/quick-deploy.sh plan standalone-client --runtime dev
./scripts/quick-deploy.sh plan managed-client --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

`plan` 不写 `--runtime` 时看的是二进制计划。`stop` 不带模式名会停掉 `.quick-deploy/` 里记录的全部模式和两种 runtime。`stop <模式>` 不带 `--runtime` 会停掉该模式的二进制和开发态。停止时按 pid 文件结束对应进程，不会按进程名去杀。
