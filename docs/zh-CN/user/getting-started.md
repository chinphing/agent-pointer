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

在仓库根目录打当前系统的安装包：

```bash
npm install
npm run build:linux
```

Windows 把第二条换成 `npm run build:windows`，macOS 换成 `npm run build:macos`。装上打出来的安装包，再打开。产物在哪见 [平台打包](../deploy/platforms.md)。

打开后到 **设置 → 模型配置 → 自定义服务** 填地址和 Key。

仓库里只想编完直接打开、不另装安装包：

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin --build
```

Windows 把 `./scripts/quick-deploy.sh` 换成 `scripts\quick-deploy.bat`。编完启动的是 `target/release/pointer-app`。

边改边看用 `npm run tauri:dev`，见 [开发第一步](../../../DEVELOPMENT.md)。

## 接着看

- [界面和输入框按键](workspace.md#输入区按键)
- [工具要不要先问你](settings.md)
- [Skills](skills.md)
- [IM 通道](im-channels.md)
