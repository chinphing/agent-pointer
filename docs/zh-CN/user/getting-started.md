# 快速上手

[English](../../en/user/getting-started.md) | 简体中文

## 1. 官网安装

1. 打开 [官网下载页](https://pointer.readflowai.com/download)，下载当前系统的安装包，安装，打开 Pointer。
2. 左下角 **账户** → **登录**。必须登录。登录后平台会下发模型。
3. 发一句「你好」。

要接自己的模型，登录后再到 **设置 → 模型配置 → 自定义服务** 填地址和 Key。未登录时，自己的 Key 不能用来发消息。详见 [配置模型服务](model-providers.md)。

登录后浏览器可能打开官网，回到窗口即可。

## 2. 社区版安装

社区版是 [GitHub Releases](https://github.com/chinphing/agent-pointer/releases) 上的安装包。下载当前系统的包，安装，打开 Pointer。

模型不会从平台下来。到 **设置 → 模型配置 → 自定义服务** 填地址和 Key，点 **测试连接**，再发一句「你好」。

和官网包的差别见 [官方包与本地构建](which-build.md)。

## 3. 自行编译安装

命令都在仓库根目录执行。个人版不要设置 `POINTER_EDITION`。企业版把示例域名换成自己的控制面。产物位置见 [平台打包](../deploy/platforms.md)。

### 客户端编译安装

#### 个人版

1. 一行命令编译并启动

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start standalone-client --runtime bin --build

# Windows
npm install && scripts\quick-deploy.bat start standalone-client --runtime bin --build
```

编完启动 `target/release/pointer-app`。模型在 **设置 → 模型配置 → 自定义服务** 里填写。

2. 编译生成安装包

```bash
# Linux
npm install && npm run build:linux

# macOS
npm install && npm run build:macos

# Windows
npm install && npm run build:windows
```

#### 企业版

1. 一行命令编译并启动

企业版客户端会给更新包签名。私钥默认路径：

- Linux / macOS：`~/.tauri/pointer-updater.key`
- Windows：`%USERPROFILE%\.tauri\pointer-updater.key`

这把密钥生成一次，以后每次编译都用同一份。换一把新密钥后，已经安装的客户端验不过更新。

在终端里执行下面的命令时，文件还不存在会问一次。输入 `y` 就在默认路径生成私钥，并在旁边写下 `pointer-updater.key.pub`。这次编译把这把公钥打进安装包，不改仓库里的配置。输入其他内容就退出，不编译。

不想等待确认时，在 `--build` 后面加上 `--generate-updater-key`。文件已经存在时不会覆盖。

没有交互终端、也没有这个参数时，命令会退出，并打印下面这条生成命令：

```bash
npm run tauri signer generate -w ~/.tauri/pointer-updater.key
```

Windows 把路径换成 `%USERPROFILE%\.tauri\pointer-updater.key`。

已经有私钥时，把绝对路径写到 `pointer.local.env` 的 `TAURI_SIGNING_PRIVATE_KEY_PATH`。开头的 `~` 会展开。私钥有密码时再写 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。脚本生成的密钥没有密码。旁边没有 `.pub` 时，安装包继续用仓库里的公钥，那把公钥必须和私钥配对。

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start managed-client --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com

# Windows
npm install && scripts\quick-deploy.bat start managed-client --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

2. 编译生成安装包

```bash
# Linux
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com POINTER_DOWNLOAD_URL=https://pointer.example.com/download TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/pointer-updater.key npm run build:linux

# macOS
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com POINTER_DOWNLOAD_URL=https://pointer.example.com/download TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/pointer-updater.key npm run build:macos

# Windows
npm install && set POINTER_EDITION=managed&& set VITE_POINTER_EDITION=managed&& set POINTER_API_BASE=https://pointer-api.example.com&& set POINTER_WEB_BASE=https://pointer.example.com&& set VITE_POINTER_WEB_BASE=https://pointer.example.com&& set COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com&& set POINTER_DOWNLOAD_URL=https://pointer.example.com/download&& set TAURI_SIGNING_PRIVATE_KEY_PATH=%USERPROFILE%\.tauri\pointer-updater.key&& npm run build:windows
```

安装包命令也一样。终端里缺私钥会询问。不想等待确认时加上参数，例如 `npm run build:linux -- --generate-updater-key`。上面的 `TAURI_SIGNING_PRIVATE_KEY_PATH` 指向已有私钥时直接使用，不会重新生成。

### Web Server 编译安装

#### 个人版

1. 一行命令编译并启动

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start standalone-server --runtime bin --build

# Windows
npm install && scripts\quick-deploy.bat start standalone-server --runtime bin --build
```

页面在 `http://127.0.0.1:8787/`。第一次启动把本地账密写到 `.quick-deploy/bin/standalone-server/standalone-admin.env`。

2. 编译生成安装包

```bash
# Linux
npm install && npm run server:build

# macOS
npm install && npm run server:build

# Windows
npm install && npm run server:build
```

#### 企业版

1. 一行命令编译并启动

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start managed-server --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com

# Windows
npm install && scripts\quick-deploy.bat start managed-server --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

2. 编译生成安装包

```bash
# Linux
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com npm run server:build

# macOS
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com npm run server:build

# Windows
npm install && set POINTER_EDITION=managed&& set VITE_POINTER_EDITION=managed&& set POINTER_API_BASE=https://pointer-api.example.com&& set POINTER_WEB_BASE=https://pointer.example.com&& set VITE_POINTER_WEB_BASE=https://pointer.example.com&& set COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com&& npm run server:build
```

## 接着看

- [界面和输入框按键](workspace.md#输入区按键)
- [工具要不要先问你](settings.md)
- [Skills](skills.md)
- [IM 通道](im-channels.md)
