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

Run every command from the repository root. A personal build leaves `POINTER_EDITION` unset. An enterprise build replaces the example domains with its own control plane. Artifact locations are in [Platform packaging](../deploy/platforms.md).

### Client

#### Personal

1. One command to compile and start

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start standalone-client --runtime bin --build

# Windows
npm install && scripts\quick-deploy.bat start standalone-client --runtime bin --build
```

This starts `target/release/pointer-app`. Fill in the model under **Settings → Models → Custom services**.

2. Build an installer

```bash
# Linux
npm install && npm run build:linux

# macOS
npm install && npm run build:macos

# Windows
npm install && npm run build:windows
```

#### Enterprise

1. One command to compile and start

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start managed-client --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com

# Windows
npm install && scripts\quick-deploy.bat start managed-client --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

This starts `target/release/pointer-app`. If the updater private key is missing, the terminal asks once; `y` creates it. The default path is `~/.tauri/pointer-updater.key` (Windows: `%USERPROFILE%\.tauri\pointer-updater.key`), and later builds keep using that file. Add `--generate-updater-key` after `--build` to create it without asking.

Generate it yourself:

```bash
# Linux / macOS
npm run tauri signer generate -w ~/.tauri/pointer-updater.key

# Windows
npm run tauri signer generate -w %USERPROFILE%\.tauri\pointer-updater.key
```

2. Build an installer

```bash
# Linux
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com POINTER_DOWNLOAD_URL=https://pointer.example.com/download TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/pointer-updater.key npm run build:linux

# macOS
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com POINTER_DOWNLOAD_URL=https://pointer.example.com/download TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/pointer-updater.key npm run build:macos

# Windows
npm install && set POINTER_EDITION=managed&& set VITE_POINTER_EDITION=managed&& set POINTER_API_BASE=https://pointer-api.example.com&& set POINTER_WEB_BASE=https://pointer.example.com&& set VITE_POINTER_WEB_BASE=https://pointer.example.com&& set COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com&& set POINTER_DOWNLOAD_URL=https://pointer.example.com/download&& set TAURI_SIGNING_PRIVATE_KEY_PATH=%USERPROFILE%\.tauri\pointer-updater.key&& npm run build:windows
```

A missing private key is handled the same way as above.

### Web Server

#### Personal

1. One command to compile and start

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start standalone-server --runtime bin --build

# Windows
npm install && scripts\quick-deploy.bat start standalone-server --runtime bin --build
```

The page is at `http://127.0.0.1:8787/`. The first start writes the local account to `.quick-deploy/bin/standalone-server/standalone-admin.env`.

2. Build an installer

```bash
# Linux
npm install && npm run server:build

# macOS
npm install && npm run server:build

# Windows
npm install && npm run server:build
```

#### Enterprise

1. One command to compile and start

```bash
# Linux / macOS
npm install && ./scripts/quick-deploy.sh start managed-server --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com

# Windows
npm install && scripts\quick-deploy.bat start managed-server --runtime bin --build --api-base https://pointer-api.example.com --web-base https://pointer.example.com --annotate-base https://pointer-som.example.com
```

2. Build an installer

```bash
# Linux
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com npm run server:build

# macOS
npm install && POINTER_EDITION=managed VITE_POINTER_EDITION=managed POINTER_API_BASE=https://pointer-api.example.com POINTER_WEB_BASE=https://pointer.example.com VITE_POINTER_WEB_BASE=https://pointer.example.com COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com npm run server:build

# Windows
npm install && set POINTER_EDITION=managed&& set VITE_POINTER_EDITION=managed&& set POINTER_API_BASE=https://pointer-api.example.com&& set POINTER_WEB_BASE=https://pointer.example.com&& set VITE_POINTER_WEB_BASE=https://pointer.example.com&& set COMPUTER_ANNOTATE_API_BASE=https://pointer-som.example.com&& npm run server:build
```

## After that

- [The window and composer keys](workspace.md#composer-keys)
- [Whether a tool asks you first](settings.md)
- [Skills](skills.md)
- [IM channels](im-channels.md)
