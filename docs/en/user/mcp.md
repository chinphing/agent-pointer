# External tool services (MCP)

English | [简体中文](../../user/mcp.md)

> For **Pointer end users**. It explains how to add an external tool service (MCP server) directly in the settings UI,
> so a conversation can call the tools that external service provides.
> For MCP servers declared inside a plugin see [`plugins.md`](plugins.md).

---

## 1. What it is

An "external tool service" (MCP server) is **a set of tools that someone else or you provide**, exposed to the AI through a unified protocol.
Once added, those tools appear in the conversation and the model can call them directly — no plugin installation needed.

Pointer is an **MCP client**: you only provide the service address (or a local launch command); it takes care of connecting,
handshaking, listing tools and calling tools.

## 2. Adding a service (configured directly in the UI)

1. Open **Settings**
2. In the left-hand list find **"MCP"** (description: external tool services)
3. Click **"Add service"** in the top right
4. Pick the connection type, fill in the fields, click **"Save"** → takes effect immediately

### Connection type 1: connect to a remote service (recommended, most common)

Suitable for using **an MCP service someone else provides** (or one you deployed on a server yourself):

| Field | Required | Description |
|------|:---:|------|
| Service name | ✅ | A name for this service, used to identify it in conversations |
| Service address | ✅ | Full URL, **the port goes inside the URL**, e.g. `http://192.168.1.10:3000/mcp`, `https://mcp.example.com/mcp` |
| Access token | Optional | The API token the provider gives you; it is sent automatically as `Authorization: Bearer <token>` |

### Connection type 2: launch a local program

Suitable for **services you develop yourself** (local scripts / executables):

| Field | Required | Description |
|------|:---:|------|
| Service name | ✅ | A name for this service |
| Launch command | ✅ | Path to the program or script, e.g. `/usr/local/bin/my-mcp` or `python3 server.py` |
| Command arguments | Optional | Space-separated, e.g. `serve --port 8080` (under "Advanced settings") |
| Environment variables | Optional | One `KEY=VALUE` per line, e.g. `TOKEN=replace-me` (under "Advanced settings") |

## 3. Managing services

- **Edit**: click the pencil icon on the right of the service row, save after editing, and the configuration updates and reconnects immediately
- **Delete**: click the bin icon on the right of the service row; the configuration is removed and the session closed
- **Refresh status**: re-fetch the current running status of each service
- **Restart services**: restart all configured services (use when a status is abnormal)

Service status meanings:

| Status | Meaning |
|------|------|
| Normal | Connected, tools available |
| Interrupted | Connection/process interrupted; recovering automatically with a backoff strategy |
| Running abnormally | Consecutive failures reached the limit and automatic recovery has stopped (click "Restart services" to recover manually) |
| Not enabled | Configured but not started |

## 4. Where the configuration lives

All configuration is stored in **local user configuration** (`user_settings.json`) and **does not depend on any server config file**.
It is restored automatically after an app restart.

## 5. FAQ

**Q: After declaring a service, do I still need to add a client?**
No. Pointer is itself the client; the service you add is the client connection configuration, and saving connects it.

**Q: How do I fill in the port?**
stdio (local program) needs no port; for remote services put the port in the URL (`http://host:port/path`).

**Q: I cannot connect to a remote service?**
Check: (1) whether the URL is complete (including the port); (2) whether the service is running and the firewall allows it; (3) whether a service that needs a token has one filled in.
The UI shows the specific error message, which you can use to troubleshoot.

**Q: How do I use an MCP service declared in a plugin?**
The plugin author declares the service in the plugin manifest (`pointer-plugin.toml`) under `[[mcp_servers.server]]`;
after you **enable the plugin** it connects automatically, with no need to add it manually in the "MCP" panel. See [`plugins.md`](plugins.md).

[Back to the documentation index](../README.md)
