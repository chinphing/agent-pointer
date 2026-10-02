# 命令行与脚本参考

[English](../../en/developer/cli.md) | 简体中文

这一页是「哪个命令在哪」的索引：服务端二进制自己的参数、仓库里的 npm scripts、部署用的独立脚本，以及**哪些包根本没有可执行文件**。

## `pointer-server` 的命令行

它**没有子命令解析器**（没有 clap），只在启动时检查几个特殊模式；命中就打印结果并退出，不命中就正常启动服务器。

| 参数 | 用法 | 输出 |
| --- | --- | --- |
| `--hash-password` | `--hash-password [--secret SECRET] [PASSWORD]` | stdout 一行 `password_hmac = "<hex>"` |
| `--mint-sso-ticket` | `--mint-sso-ticket --sub USER_ID [--name NICK] [--ttl SECS] [--secret SECRET] [--audience AUD]` | stdout 一行 ticket |
| `--machine-id-json` | 无参数 | stdout 机器绑定信息（pretty JSON） |
| `--machine-id` | 无参数 | stdout 主绑定 token（`fp1:…`） |

**没有 `--help`、`--version`、`--config`。** 这几个参数不会被识别 —— 比如 `pointer-server --version` 不会打印版本，而是**正常启动服务器**。运行期参数（监听地址、配置文件路径等）全部走环境变量，见 [独立部署开发文档](standalone-deployment.md) 的配置参考。

服务端模式下**多余的参数会被静默忽略**；只有 `--hash-password` 与 `--mint-sso-ticket` 会对不认识的参数报 `error: unexpected argument`。

### 生成密码摘要

```bash
pointer-server --hash-password --secret '<hmac_secret>' '<password>'
# → password_hmac = "…"
```

- `--secret` 取值优先级：`--secret` 参数 → 环境变量 `POINTER_SERVER_AUTH_HMAC_SECRET` → 加载配置文件后再读同一个环境变量
- 密码可以写成位置参数；**省略则从 stdin 读到 EOF**（方便脚本里 `echo` 管道），会自动去掉尾部换行
- 密码为空报错，退出码 1

### 签发 SSO 短时票

```bash
pointer-server --mint-sso-ticket --sub 1001 --name '张三' --ttl 300
```

- `--sub` 必填；`--secret` / `--audience` 缺省时读 `POINTER_SERVER_SSO_SECRET` / `POINTER_SERVER_SSO_AUDIENCE`
- `--ttl` 默认 **120** 秒；值非法时**静默回退 120**（不报错）
- 用途与验签见 [standalone 本地登录](standalone-local-login.md)

### 机器指纹

```bash
pointer-server --machine-id-json   # 推荐：完整绑定信息，交给签发方
pointer-server --machine-id        # 只要 fp1:… token
```

两个命令都**不需要**配置或 License，可以在安装后立刻执行。

## npm scripts

`package.json` 里共 39 个脚本，按用途分七组。

### 开发

| script | 命令 | 用途 |
| --- | --- | --- |
| `dev` | `vite` | 前端 dev server（Tauri 的 `devUrl` 用 1420） |
| `web:dev` | `vite --host 0.0.0.0 --port 1420` | 局域网 / 容器内访问前端 |
| `tauri:dev` | `node scripts/tauri-dev.mjs` | 读 `pointer.local.env` 后跑 `tauri dev` |
| `server:dev` | `cargo run -p pointer-server` | debug 前台运行服务端 |
| `server:start` | `cargo run -p pointer-server --release` | release 前台运行（不写 PID 文件） |
| `preview` | `vite preview` | 预览前端 `dist/` |
| `test` / `test:watch` | `vitest run` / `vitest` | 单元测试 |
| `tauri` | `tauri` | 透传 Tauri CLI（供脚本 / CI 调用） |

### 构建

| script | 命令 | 用途 |
| --- | --- | --- |
| `build` | `vue-tsc --noEmit && vite build` | 类型检查 + 前端生产构建 |
| `tauri:build` | `node scripts/tauri-build.mjs` | 跨平台桌面打包（含签名私钥、Linux 特例） |
| `build:windows` / `build:macos` / `build:linux` | `npm run tauri:build` | 平台别名 |
| `build:macos:universal` | `… --target universal-apple-darwin` | 通用二进制 |
| `build:macos:signed` / `build:macos:sign-only` | `node scripts/macos-signed-build.mjs` | 读 `signing/macos/signing.env` 做签名（+ 公证） |
| `signing:macos:setup` | `node scripts/macos-signing-setup.mjs` | 交互式生成签名配置 |
| `build:linux:appimage` | `… --bundles appimage` | 只出 AppImage |
| `server:build` | `node scripts/build-server.mjs` | 构建服务端 + 打包 zip（Linux 有 `dpkg-deb` 时再出 `.deb`） |
| `server:package` | `… --package-only` | 只用现有产物打 zip（跳过编译） |
| `server:build:deb` | `… --deb-only` | 只打 `.deb` |
| `server:build:linux` | `npm run server:build` | 别名 |
| `server:deb` | `npm run server:build` | 别名（名字有误导，见下） |

> ⚠️ `server:deb` 也是 `npm run server:build` 的**别名**，并不会「只打 deb」。要只打 deb 用 `server:build:deb`。

### 服务端进程管理

后台运行的是 `target/release/pointer-server`，PID 与日志由脚本管理：

| script | 用途 |
| --- | --- |
| `server:daemon` | 后台启动，写 PID + 日志 |
| `server:stop` | 停止（SIGTERM → 10 秒 → SIGKILL；Windows 用 `taskkill /T /F`） |
| `server:restart` | 停止后重启 |
| `server:status` | 查看状态；**已停止时退出码 1** |

### 版本与 License

| script | 用途 |
| --- | --- |
| `version:sync` | 以根 `VERSION` 为单一真源，同步 package.json / tauri.conf.json / workspace / `appVersion.ts` |
| `version:check` | 检查版本漂移，漂移则退出码 1 |
| `license-gen:dev` | 源码运行签发工具：`npm run license-gen:dev -- sign --private-key …` |
| `license-gen:build` | 编译并打包签发工具（zip） |
| `license-gen:package` | `license-gen:build` 的别名 |

License 的字段与签发流程见 [独立部署开发文档](standalone-deployment.md)。

### 文档

| script | 用途 |
| --- | --- |
| `docs:install` | 安装文档站依赖（`npm --prefix docs-site install`） |
| `docs:dev` | VitePress 本地预览 |
| `docs:build` | 构建文档站 |
| `docs:preview` | 预览构建产物 |

### 其他

| script | 用途 |
| --- | --- |
| `licenses` | 重新生成 `THIRD-PARTY-NOTICES.md` |
| `icons` | 从源 PNG 生成应用图标 |

**没有挂到 npm script 的仓库脚本**（CI 直接 `node scripts/…` 调用）：`scripts/check-doc-links.mjs`、`scripts/check-vue-template-imports.mjs`、`scripts/sync-web-icons.mjs`。

## 部署脚本：`server/scripts/`

`{start,stop,restart,status}.{sh,ps1}` 八个文件，**不接受任何参数**，PID 写 `.pointer-server.pid`、日志写 `pointer-server.log`，都在**脚本所在目录**（也就是部署目录 / `target/release`）。打包时它们会被复制到发布目录（`.sh` 会 `chmod 755`）。

| 脚本 | 行为 |
| --- | --- |
| `start` | 校验同目录 `./pointer-server` 存在；已在运行则退出码 0；否则 `nohup` 起进程并写 PID |
| `stop` | 无 PID / 进程已不存在 → 退出码 0；否则 SIGTERM，轮询 10 秒后 SIGKILL，删 PID |
| `restart` | 依次调用同目录的 `stop` + `start` |
| `status` | 运行中 → 退出码 0；已停止 / PID 过期 → 打印状态并**退出码 1** |

Windows 的 `.ps1` 逻辑相同，差别是 `stop` 用 `Stop-Process -Force`（不做优雅等待）。

## 构建环境脚本

`scripts/install-linux-build-deps.sh` —— **仅 Ubuntu / Debian**（没有 `apt-get` 直接退出 1）。用 `sudo apt-get` 安装 WebKitGTK 4.1 dev、GTK3、GStreamer、`libfuse2`、`squashfs-tools`、`patchelf`、`zsync`、`libxdo-dev`、`libssl-dev`、appindicator、pipewire/spa、`libclang-dev`、`libgbm` / `libegl` / `libdrm`、`libwayland-dev` 等，最后用 `pkg-config` 自检 `glib-2.0` / `gtk+-3.0` / `gdk-pixbuf-2.0` / `librsvg-2.0`，缺任一项退出 1。

## 哪些包有可执行文件

| 包 | 可执行文件 | 说明 |
| --- | --- | --- |
| `server/`（`pointer-server`） | ✅ `pointer-server` | 默认 bin（靠 `src/main.rs` + 包名） |
| `tools/license-gen`（`pointer-license-gen`） | ✅ `license-gen` | 唯一显式声明 `[[bin]]` 的包 |
| `src-tauri/`（`pointer-app`） | ✅ `pointer-app` | Tauri 打包，桌面产物名取 `productName`（`Pointer`） |
| `crates/pointer-core` | ❌ | **纯 lib**，没有 `src/main.rs`，不能直接运行 |
| `crates/pointer-channels` | ❌ | **纯 lib** |

所以想「跑一下 pointer-core」是做不到的：它只能被 `pointer-server`、`pointer-app` 或测试链接。

## 常见问题

**`pointer-server --version` 没反应 / 直接启动了服务**

它不认 `--version`。要确认版本看 `[server]` 启动日志或 `/api/version`，不要在命令行上找。

**`pointer-server --help`**

同上，没有帮助输出；本文与 [独立部署开发文档](standalone-deployment.md) 就是它的参数说明。

**`npm run server:deb` 出了一堆东西**

名字有误导：它是 `server:build` 的别名（会编译并打包 zip，Linux 上顺带出 deb）。只要 deb 用 `server:build:deb`。

**`npm run server:status` 返回非 0**

「已停止」和「PID 过期」都返回 1。这是**有意的**，方便脚本里直接判断。

**`npm run license-gen:dev -- sign` 报参数缺失**

`license-gen:dev` 后面的 `--` 是必须的，它把参数透传给 `cargo run`。至少要有 `--private-key` 和 `--customer-id`。

**`scripts/install-linux-build-deps.sh` 在 CentOS 上失败**

它只支持 Ubuntu / Debian（依赖 `apt-get`）。其他发行版按脚本里的包清单手工装对应依赖。

## 相关

- [独立部署开发文档](standalone-deployment.md) —— 配置文件、License、API 端点
- [standalone 本地登录](standalone-local-login.md) —— SSO 票与账号密码登录
- [跨平台开发与打包](../contributing/cross-platform-build.md) —— 三平台构建与打包细节
