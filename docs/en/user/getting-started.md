# Getting started

English | [简体中文](../../zh-CN/user/getting-started.md)

## Use it now

1. Open the [download page](https://pointer.readflowai.com/download), install, and open Pointer.
2. Bottom-left **Account** → **Sign in**. After signing in, the platform delivers a model.
3. Send "Hello".

Without a platform account, open **Settings → Models → Custom services**, fill in the address and Key, and click **Test connection**. See [Configuring model services](model-providers.md).

After signing in, the browser may open the website. Come back to the window.

When the installer did not come from the website:

- [An installer from your company](https://github.com/chinphing/agent-pointer/releases)
- [Open the app from this repository](../deploy/quick-start.md)
- [Open a server in the browser](standalone-server.md)

## Changing the code

From the repository root:

```bash
npm install
npm run tauri:dev
```

You can edit once the window is up. The first compile can take a few minutes. If this step fails, read the [first development step](../DEVELOPMENT.md).

## After that

- [The window and composer keys](workspace.md#composer-keys)
- [Whether a tool asks you first](settings.md)
- [Skills](skills.md)
- [IM channels](im-channels.md)
- [Official packages and local builds](which-build.md)
