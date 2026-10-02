# Contributor guide
English | [简体中文](../../zh-CN/contributing/README.md)

For **contributors to this repository and packaging maintainers**: building from source, platform-specific UI, internal evaluation and more.

| Document | Description |
|------|------|
| [**editions.md**](../deploy/editions.md) | **Packaging flavour × runtime form, the four cells** (managed / standalone × client / server): build commands / variables / artifacts / verification / external dependencies (start with the [rework design](../design/control-plane-and-editions.md)) |
| [**cross-platform-build.md**](cross-platform-build.md) | **Windows / macOS / Linux development and packaging** (environment, commands, artifacts, CI) |
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
