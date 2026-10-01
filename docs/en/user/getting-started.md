# Getting started

English | [简体中文](../../zh-CN/user/getting-started.md)

First decide whether you are using the [official build or a local build](which-build.md).

## Official build

1. Download the installer for your platform from the [official download page](https://pointer.readflowai.com/download) (Windows / macOS / Linux)
2. Open **Settings** and fill in the model API Key (by default you can use the Qwen DashScope compatible endpoint)
3. Click **Test connection**
4. Send a message, for example "Hello"

Only after signing in to the official website under **Settings → Account** can you use the cloud host and top up. A successful desktop OAuth may redirect to the official website.

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

## Next steps

- [Using Skills](skills.md)
- [IM channels](im-channels.md)
- [Cloud host](cloud-host.md) (official builds only)
