# Contributor guide
English | [简体中文](../../zh-CN/contributing/README.md)

For **contributors to this repository and packaging maintainers**. To change the client, run `npm install` and then `npm run tauri:dev` from the repository root. To change the Web Server, run `npm run server:dev` and `npm run web:dev` in two terminals. Read the topics below after it is up. Start an already built program with the script in [Quick start](../deploy/quick-start.md).

| Document | Description |
|------|------|
| [**DEVELOPMENT.md**](../DEVELOPMENT.md) | The first-step command, and the environment, debugging, and tests for when it fails |
| [**cross-platform-build.md**](cross-platform-build.md) | **Windows / macOS / Linux development and packaging** (environment, commands, artifacts, CI) |
| [**editions.md**](../deploy/editions.md) | The four-cell checklist for when you are building an installer: build commands / variables / artifacts / verification / external dependencies. Background: [rework design](../design/control-plane-and-editions.md) |
| [**versioning.md**](versioning.md) | **Single source of truth for the version number** (`VERSION` + `npm run version:sync`) |
| [**ci.md**](ci.md) | **CI and release flow**: the four workflows, reproducing them locally, the pre-PR checklist, DCO and Dependabot |
| [**docs-site.md**](docs-site.md) | **Docs site rules**: tiers, `SITE_EXCLUDES`, sidebar and routes, preview and link checks |
| [**macos-window-chrome.md**](macos-window-chrome.md) | **macOS traffic lights and top-bar alignment** (reapply/repair, compact mode, constant sync, troubleshooting) |
| [../../ui/visual-theme.md](../../zh-CN/ui/visual-theme.md) | **UI colour tokens** (greyscale surfaces + system blue; `slate-*` / hard-coded hex are forbidden; except Diff) |
| [web-media-and-desktop-snapshot.md](web-media-and-desktop-snapshot.md) | Web media and desktop snapshots |
| [coder-agent-offline-eval-setup.md](coder-agent-offline-eval-setup.md) | Coder offline evaluation environment setup |

> `macos-window-chrome.md` and `web-media-and-desktop-snapshot.md` are **UI implementation** docs; the sidebar lists them under **Internals → UI notes** (placement by config, files never move — see [docs site rules](docs-site.md)).

For the local development workflow see **[`DEVELOPMENT.md`](../DEVELOPMENT.md)**.

For user documentation see **[`../user/`](../user/README.md)**; for external integration see **[`../developer/`](../developer/README.md)**.

[Back to the documentation index](../README.md)
