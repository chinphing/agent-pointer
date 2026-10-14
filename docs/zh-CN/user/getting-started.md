# 快速上手

[English](../../en/user/getting-started.md) | 简体中文

## 现在就用

1. 打开 [官网下载页](https://pointer.readflowai.com/download)，安装，打开 Pointer。
2. 左下角 **账户** → **登录**。登录后平台会下发模型。
3. 发一句「你好」。

还没有平台账号时，到 **设置 → 模型配置 → 自定义服务** 填地址和 Key，点 **测试连接**。详见 [配置模型服务](model-providers.md)。

登录后浏览器可能打开官网，回到窗口即可。

安装包不是从官网来的：

- [公司发来的安装包](https://github.com/chinphing/agent-pointer/releases)
- [在这份仓库里打开程序](../deploy/quick-start.md)
- [用浏览器打开一台服务](standalone-server.md)

## 要改代码

在仓库根目录执行：

```bash
npm install
npm run tauri:dev
```

窗口出现后就可以改。第一次编译要几分钟。这一步失败时再看 [开发第一步](../../../DEVELOPMENT.md)。

## 接着看

- [界面和输入框按键](workspace.md#输入区按键)
- [工具要不要先问你](settings.md)
- [Skills](skills.md)
- [IM 通道](im-channels.md)
- [官方包和本地构建](which-build.md)
