# Getting started

English | [简体中文](../../zh-CN/user/getting-started.md)

## Use it now

1. Open the [download page](https://pointer.readflowai.com/download), install, and open Pointer.
2. Bottom-left **Account** → **Sign in**. After signing in, the platform delivers a model.
3. Send "Hello".

Without a platform account, open **Settings → Models → Custom services**, fill in the Base URL and API Key, and click **Test connection**. See [Configuring model services](model-providers.md).

After signing in you can also use the cloud host and top up. A successful desktop sign-in may open the website; come back to the window.

## When you did not install from the website

The [difference between an official build and a local build](which-build.md) can wait. Installers distributed inside a company are on [GitHub Releases](https://github.com/chinphing/agent-pointer/releases). When the repository already has a build, start the binary with the script in [Start locally](../deploy/quick-start.md). To change the code, use npm and follow the [first development step](../DEVELOPMENT.md). To open an already deployed server in a browser, see [standalone-server.md](standalone-server.md).

A local build has no built-in official website domain. The model address and Key are both filled in yourself in settings.

## Desktop and web

| Entry point | Description |
|------|------|
| Desktop | Tauri client with local storage and the full capability set (including IM channels, computer control, …) |
| Web | Access a deployed `pointer-server` in a browser; shares the same conversation logic as the desktop |

## Tools and confirmation

- Tool calls are **allowed automatically** by default; you can change this in settings so that sensitive tools need a second confirmation
- Web search (`web_search`) needs a valid search/model configuration and is billed by the provider

## Keyboard shortcuts

These keys in the composer are worth remembering:

| Key | Behaviour |
| --- | --- |
| `Enter` | Send. While a response is generating, it is **queued** and sent after the current turn ends |
| `Shift+Enter` | New line |
| `⌘/Ctrl+Enter` | **Soft-cancel the current turn and send immediately** |
| `Enter` (composer empty, with a queued message) | Send the head of the queue immediately |
| `⌘/Ctrl+F` | In-page search. When focus is in the workspace panel, it goes to the file tree or the file preview first |
| `Esc` | Close search / rename / dialogs / image preview |

`⌘/Ctrl+Enter` is the easiest one to get wrong: it is not "new line", and it is not an ordinary "send" either — it **first stops the turn that is running** (only the synchronous turn; background jobs keep going) and then sends what you have typed right away — in effect "interrupt + cut in", with the same feel as in Cursor.

## Next steps

- [Using Skills](skills.md)
- [IM channels](im-channels.md)
