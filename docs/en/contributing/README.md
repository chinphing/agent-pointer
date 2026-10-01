# Contributor guide (contributing)

English | [简体中文](../../zh-CN/contributing/README.md)

For **contributors to this repository and packaging maintainers**: building from source, platform-specific UI, internal evaluation and more.

| Document | Description |
|------|------|
| [**editions.md**](../deploy/editions.md) | **Packaging flavour × runtime form, the four cells** (managed / standalone × client / server): build commands / variables / artifacts / verification / external dependencies (start with the [rework design](../design/control-plane-and-editions.md)) |
| [**cross-platform-build.md**](cross-platform-build.md) | **Windows / macOS / Linux development and packaging** (environment, commands, artifacts, CI) |
| [**versioning.md**](versioning.md) | **Single source of truth for the version number** (`VERSION` + `npm run version:sync`) |
| [**macos-window-chrome.md**](../../zh-CN/contributing/macos-window-chrome.md) | **macOS traffic lights and top-bar alignment** (reapply/repair, compact mode, constant sync, troubleshooting) |
| [../../ui/visual-theme.md](../../zh-CN/ui/visual-theme.md) | **UI colour tokens** (greyscale surfaces + system blue; `slate-*` / hard-coded hex are forbidden; except Diff) |
| [web-media-and-desktop-snapshot.md](../../zh-CN/contributing/web-media-and-desktop-snapshot.md) | Web media and desktop snapshots |
| [coder-agent-offline-eval-setup.md](../../zh-CN/contributing/coder-agent-offline-eval-setup.md) | Coder offline evaluation environment setup |

For the local development workflow see **[`DEVELOPMENT.md`](../DEVELOPMENT.md)**.

For user documentation see **[`../user/`](../user/README.md)**; for external integration see **[`../developer/`](../developer/README.md)**.

[Back to the documentation index](../README.md)
