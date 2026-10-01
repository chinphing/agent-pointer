# Version management

English | [简体中文](../../zh-CN/contributing/versioning.md)

The application version has a single source of truth: the repository root **`VERSION`** file (currently e.g. `0.1.2`).

When having an AI / agent bump the version: change only `VERSION`, then **you must** run `npm run version:sync` (see `.cursor/rules/versioning.mdc`). Do not hand-edit version literals elsewhere.

## Bumping the version

1. Edit the root `VERSION` (only this one place)
2. Run the sync:

```bash
npm run version:sync
```

3. Confirm there is no drift:

```bash
npm run version:check
```

## Where it is synced to

| Target | Purpose |
|------|------|
| `package.json` / `package-lock.json` | npm / frontend package version |
| `src-tauri/tauri.conf.json` | Tauri installer and `getVersion()`; `build.rs` injects `POINTER_APP_VERSION` |
| `Cargo.toml` `[workspace.package].version` | The Rust crates inherit it via `version.workspace = true` |
| `src/lib/appVersion.ts` | Frontend fallback display (generated file, do not hand-edit) |

For other references use derived values instead of hard-coding literals again:

- Rust runtime: `pointer_core::client_env::app_version()` (prefers `POINTER_APP_VERSION`)
- Rust crates: `env!("CARGO_PKG_VERSION")`
- deb packaging: `scripts/build-server-deb.mjs` reads `package.json`

Example version numbers in design documents (e.g. auto-update use cases) may keep their historical digits; they need not be updated every time.
