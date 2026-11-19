# Command-line and script reference

English | [简体中文](../../zh-CN/developer/cli.md)

This page is the "which command lives where" index: the server binary's own arguments, the repository's npm scripts, the deployment scripts, and **which packages have no executable at all**.

## The `pointer-server` command line

It has **no general subcommand parser** (no clap); at startup it only checks a few special modes — plus the single subcommand form `auth doctor`. If one matches it prints the result and exits; otherwise it starts the server normally.

| Argument | Usage | Output |
| --- | --- | --- |
| `auth doctor` | `auth doctor [--config PATH] [--network] [--exchange]` | the read-only login preflight report; **exit code 1 when any check FAILs** |
| `--hash-password` | `--hash-password [--secret SECRET] [PASSWORD]` | one line on stdout: `password_hmac = "<hex>"` |
| `--mint-sso-ticket` | `--mint-sso-ticket --sub USER_ID [--name NICK] [--ttl SECS] [--secret SECRET] [--audience AUD]` | one line on stdout: the ticket |
| `--machine-id-json` | no arguments | machine binding information on stdout (pretty JSON) |
| `--machine-id` | no arguments | the primary binding token on stdout (`fp1:…`) |

**There is no `--help` or `--version`.** These arguments are not recognised — for example `pointer-server --version` does not print a version, it **starts the server normally**. A `--config` flag is accepted only after `auth doctor`; everywhere else the configuration file path comes from `POINTER_SERVER_CONFIG`. All runtime arguments (listen address, configuration file path, …) go through environment variables; see the configuration reference in the [standalone deployment developer documentation](standalone-deployment.md).

In server mode **extra arguments are silently ignored**; only `auth doctor`, `--hash-password` and `--mint-sso-ticket` report `error: unexpected argument` for arguments they do not recognise.

### Generating a password digest

```bash
pointer-server --hash-password --secret '<hmac_secret>' '<password>'
# → password_hmac = "…"
```

- `--secret` resolution order: the `--secret` argument → the `POINTER_SERVER_AUTH_HMAC_SECRET` environment variable → the same environment variable read again after the configuration file is loaded
- The password can be passed as a positional argument; **when omitted it is read from stdin until EOF** (handy for `echo` pipes in scripts), and the trailing newline is stripped automatically
- An empty password is an error and exits with code 1

### Minting an SSO short-lived ticket

```bash
pointer-server --mint-sso-ticket --sub 1001 --name '张三' --ttl 300
```

- `--sub` is required; when `--secret` / `--audience` are omitted they fall back to `POINTER_SERVER_SSO_SECRET` / `POINTER_SERVER_SSO_AUDIENCE`
- `--ttl` defaults to **120** seconds; an invalid value **silently falls back to 120** (no error)
- For usage and signature verification see [standalone local login](standalone-local-login.md)

### Machine fingerprint

```bash
pointer-server --machine-id-json   # recommended: full binding information, hand it to the issuer
pointer-server --machine-id        # just the fp1:… token
```

Neither command **needs** configuration or a License, so they can be run right after installation.

### Login preflight: `auth doctor`

```bash
pointer-server auth doctor                      # normal config discovery
pointer-server auth doctor --config /etc/pointer-server/pointer-server.toml
pointer-server auth doctor --network            # also TCP-check the provider endpoints
pointer-server auth doctor --exchange           # also send one fake-code token request per provider
```

- **Read-only**: it never writes configuration and never touches the session store, so it is safe on a live instance
- Every check prints item → verdict → fix hint (the report text is Chinese), then a summary line `汇总：OK n · WARN n · FAIL n`
- **Exit code 1 when any check FAILs**, 0 otherwise — usable directly in CI or a deploy script; `--config` sets `POINTER_SERVER_CONFIG` for that run, an unknown argument exits 1
- Checks: configuration file parse, deployment mode and the injected default provider, provider registry validation, whether the default provider is usable, `public_url` scheme, callback-path conflicts, and a note that the session store is in-memory (single instance)
- **Offline by default**; `--network` adds a TCP connectivity check against the `authorize` / `token` / `userinfo` / lookup endpoints
- `--exchange` goes one step further: for every `oauth2_userinfo` / `oidc` provider that has a `token.url` it sends **one token request with a deliberately fake code** (10 s timeout) and classifies the answer:
  - a standard OAuth error (`invalid_grant` / `invalid_client` / `invalid_request` / `unsupported_grant_type` / `invalid_scope` / `unauthorized_client`) → **OK**: the endpoint and the client credentials work
  - an HTTP response without one of those codes → **WARN**: check `token.url` / `client_id` / `client_secret`
  - network, TLS, timeout or an unparsable body → **FAIL**
  - the endpoint hands back a token for the fake code → **WARN** (that address is probably not an OAuth token endpoint)
- `--exchange` needs `public_url` to build the `redirect_uri`; a provider without it is reported as a WARN, not a FAIL

## npm scripts

`package.json` contains 39 scripts in total, grouped into seven categories by purpose.

### Development

| script | command | purpose |
| --- | --- | --- |
| `dev` | `vite` | frontend dev server (Tauri's `devUrl` uses 1420) |
| `web:dev` | `vite --host 0.0.0.0 --port 1420` | reach the frontend from the LAN / inside a container |
| `tauri:dev` | `node scripts/tauri-dev.mjs` | reads `pointer.local.env` then runs `tauri dev` |
| `server:dev` | `cargo run -p pointer-server` | run the server in the foreground in debug |
| `server:start` | `cargo run -p pointer-server --release` | run in the foreground in release (no PID file) |
| `preview` | `vite preview` | preview the frontend `dist/` |
| `test` / `test:watch` | `vitest run` / `vitest` | unit tests |
| `tauri` | `tauri` | passthrough to the Tauri CLI (for scripts / CI) |

### Build

| script | command | purpose |
| --- | --- | --- |
| `build` | `vue-tsc --noEmit && vite build` | type check + frontend production build |
| `tauri:build` | `node scripts/tauri-build.mjs` | cross-platform desktop packaging (including the signing private key and the Linux special cases) |
| `build:windows` / `build:macos` / `build:linux` | `npm run tauri:build` | platform aliases |
| `build:macos:universal` | `… --target universal-apple-darwin` | universal binary |
| `build:macos:signed` / `build:macos:sign-only` | `node scripts/macos-signed-build.mjs` | reads `signing/macos/signing.env` and signs (+ notarizes) |
| `signing:macos:setup` | `node scripts/macos-signing-setup.mjs` | interactively generate the signing configuration |
| `build:linux:appimage` | `… --bundles appimage` | AppImage only |
| `server:build` | `node scripts/build-server.mjs` | build the server + package a zip (also emits a `.deb` on Linux when `dpkg-deb` is available) |
| `server:package` | `… --package-only` | zip the existing artifacts only (skip compilation) |
| `server:build:deb` | `… --deb-only` | build only the `.deb` |
| `server:build:linux` | `npm run server:build` | alias |
| `server:deb` | `npm run server:build` | alias (misleading name, see below) |

> ⚠️ `server:deb` is also an **alias** for `npm run server:build` and does **not** "build only the deb". To build only the deb, use `server:build:deb`.

### Server process management

The background process is `target/release/pointer-server`; the PID and logs are managed by the scripts:

| script | purpose |
| --- | --- |
| `server:daemon` | start in the background, write PID + log |
| `server:stop` | stop (SIGTERM → 10 seconds → SIGKILL; on Windows `taskkill /T /F`) |
| `server:restart` | stop and then restart |
| `server:status` | show the status; **exit code 1 when stopped** |

### Version and License

| script | purpose |
| --- | --- |
| `version:sync` | use the root `VERSION` as the single source of truth and sync package.json / tauri.conf.json / workspace / `appVersion.ts` |
| `version:check` | check for version drift, exit code 1 on drift |
| `license-gen:dev` | run the issuance tool from source: `npm run license-gen:dev -- sign --private-key …` |
| `license-gen:build` | compile and package the issuance tool (zip) |
| `license-gen:package` | alias for `license-gen:build` |

For the License fields and the issuance flow see the [standalone deployment developer documentation](standalone-deployment.md).

### Documentation

| script | purpose |
| --- | --- |
| `docs:install` | install the docs-site dependencies (`npm --prefix docs-site install`) |
| `docs:dev` | local VitePress preview |
| `docs:build` | build the docs site |
| `docs:preview` | preview the build output |

### Other

| script | purpose |
| --- | --- |
| `licenses` | regenerate `THIRD-PARTY-NOTICES.md` |
| `icons` | generate the app icons from the source PNG |

**Repository scripts that are not wired into an npm script** (CI calls them directly as `node scripts/…`): `scripts/check-doc-links.mjs`, `scripts/check-vue-template-imports.mjs`, `scripts/sync-web-icons.mjs`.

## Deployment scripts: `server/scripts/`

Eight files, `{start,stop,restart,status}.{sh,ps1}`, that **accept no arguments**; the PID is written to `.pointer-server.pid` and the log to `pointer-server.log`, both in the **script's own directory** (that is, the deployment directory / `target/release`). When packaging they are copied into the release directory (the `.sh` files are `chmod 755`ed).

| script | behaviour |
| --- | --- |
| `start` | verify that `./pointer-server` exists in the same directory; exit code 0 if it is already running; otherwise `nohup` the process and write the PID |
| `stop` | no PID / the process no longer exists → exit code 0; otherwise SIGTERM, poll for 10 seconds, then SIGKILL and remove the PID |
| `restart` | call `stop` + `start` from the same directory in order |
| `status` | running → exit code 0; stopped / stale PID → print the status and **exit code 1** |

The Windows `.ps1` scripts have the same logic, except that `stop` uses `Stop-Process -Force` (no graceful wait).

## Build environment scripts

`scripts/install-linux-build-deps.sh` — **Ubuntu / Debian only** (exits 1 immediately without `apt-get`). It uses `sudo apt-get` to install WebKitGTK 4.1 dev, GTK3, GStreamer, `libfuse2`, `squashfs-tools`, `patchelf`, `zsync`, `libxdo-dev`, `libssl-dev`, appindicator, pipewire/spa, `libclang-dev`, `libgbm` / `libegl` / `libdrm`, `libwayland-dev` and more, then self-checks `glib-2.0` / `gtk+-3.0` / `gdk-pixbuf-2.0` / `librsvg-2.0` with `pkg-config`, exiting 1 if any of them is missing.

## Which packages have executables

| package | executable | notes |
| --- | --- | --- |
| `server/` (`pointer-server`) | ✅ `pointer-server` | default bin (via `src/main.rs` + the package name) |
| `tools/license-gen` (`pointer-license-gen`) | ✅ `license-gen` | the only package that declares `[[bin]]` explicitly |
| `src-tauri/` (`pointer-app`) | ✅ `pointer-app` | packaged by Tauri; the desktop artifact name comes from `productName` (`Pointer`) |
| `crates/pointer-core` | ❌ | **pure lib**, no `src/main.rs`, cannot be run directly |
| `crates/pointer-channels` | ❌ | **pure lib** |

So "just run pointer-core" is not possible: it can only be linked by `pointer-server`, `pointer-app` or tests.

## FAQ

**`pointer-server --version` does nothing / starts the service**

It does not recognise `--version`. To confirm the version, look at the `[server]` startup log or `/api/version`; do not look for it on the command line.

**`pointer-server --help`**

Same as above: there is no help output; this document and the [standalone deployment developer documentation](standalone-deployment.md) are its argument reference.

**`npm run server:deb` produced a pile of things**

The name is misleading: it is an alias for `server:build` (it compiles and packages a zip, and also emits a deb on Linux). To build only the deb, use `server:build:deb`.

**`npm run server:status` returns non-zero**

Both "stopped" and "stale PID" return 1. This is **intentional**, so that scripts can branch on it directly.

**`npm run license-gen:dev -- sign` reports missing arguments**

The `--` after `license-gen:dev` is required; it forwards the arguments to `cargo run`. At least `--private-key` and `--customer-id` are needed.

**`scripts/install-linux-build-deps.sh` fails on CentOS**

It only supports Ubuntu / Debian (it depends on `apt-get`). On other distributions, install the corresponding dependencies by hand from the package list in the script.

## Related

- [Standalone deployment developer documentation](standalone-deployment.md) — configuration file, License, API endpoints
- [standalone local login](standalone-local-login.md) — SSO ticket and username/password login
- [Cross-platform development and packaging](../contributing/cross-platform-build.md) — build and packaging details for the three platforms
