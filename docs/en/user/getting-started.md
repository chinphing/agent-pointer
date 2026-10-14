# Getting started

English | [简体中文](../../zh-CN/user/getting-started.md)

## 1. Install from the website

1. Open the [download page](https://pointer.readflowai.com/download), download the installer for this system, install it, and open Pointer.
2. Bottom-left **Account** → **Sign in**. Sign-in is required. After signing in, the platform delivers a model.
3. Send "Hello".

To use your own model, sign in first, then open **Settings → Models → Custom services** and fill in the address and Key. Until you are signed in, your own Key cannot send a message. See [Configuring model services](model-providers.md).

After signing in, the browser may open the website. Come back to the window.

## 2. Install the community build

The community build is the installer on [GitHub Releases](https://github.com/chinphing/agent-pointer/releases). Download the package for this system, install it, and open Pointer.

The platform does not deliver a model. Open **Settings → Models → Custom services**, fill in the address and Key, click **Test connection**, and send "Hello".

How this differs from the website package: [Official packages vs. local builds](which-build.md).

## 3. Compile and install it yourself

From the repository root, build the installer for this system:

```bash
npm install
npm run build:linux
```

On Windows replace the second command with `npm run build:windows`. On macOS use `npm run build:macos`. Install the package that build produces, then open it. Artifact locations are in [Platform packaging](../deploy/platforms.md).

After it opens, fill in the address and Key under **Settings → Models → Custom services**.

To compile and open the app from the repository without making a separate installer:

```bash
./scripts/quick-deploy.sh start standalone-client --runtime bin --build
```

On Windows replace `./scripts/quick-deploy.sh` with `scripts\quick-deploy.bat`. The process that starts is `target/release/pointer-app`.

To edit while it runs, use `npm run tauri:dev`. See the [first development step](../DEVELOPMENT.md).

## After that

- [The window and composer keys](workspace.md#composer-keys)
- [Whether a tool asks you first](settings.md)
- [Skills](skills.md)
- [IM channels](im-channels.md)
