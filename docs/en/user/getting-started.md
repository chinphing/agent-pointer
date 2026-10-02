# Getting started

English | [简体中文](../../zh-CN/user/getting-started.md)

First decide whether you are using the [official build or a local build](which-build.md).

## Official build

1. Download the installer for your platform from the [official download page](https://pointer.readflowai.com/download) (Windows / macOS / Linux)
2. **Configure a model**, one of the two:
   - **Sign in to a platform account** (bottom-left **Account** menu → **Sign in**): after signing in the platform delivers model services, so you can use them straight away
   - **Set it up yourself**: under **Settings → Models → Custom services**, fill in the Base URL and API Key, then click **Test connection**. See [Configuring model services](model-providers.md)
3. Send a message, for example "Hello"

After signing in to a platform account you can also use the cloud host and top up. A successful desktop OAuth may redirect to the official website.

## Local build / self-hosted Web

For enterprise deployment, get the installer from [GitHub Releases](https://github.com/chinphing/agent-pointer/releases). To run from source see [DEVELOPMENT.md](../DEVELOPMENT.md). To deploy `pointer-server` and access it with a browser see [standalone-server.md](standalone-server.md).

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
