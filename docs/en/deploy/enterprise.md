# Enterprise Internal Deployment

English | [简体中文](../../zh-CN/deploy/enterprise.md)

This page covers **deploying the whole set together**: the control plane plus the client (or the server).

Enterprise internal deployment is not "install one program". It is making three things recognise each other — the control plane owns accounts and releases, the client is what people use daily, and the server (optional) shares a model gateway across the team. Skip any one of them and you get a half-working state: you can log in but updates never arrive, or you can chat but the balance link goes nowhere.

To build a single artifact, see [Packaging and Deployment](README.md). This page is about wiring them together.

---

## 1. The three roles

```
                    ┌──────────────────────┐
                    │  control plane       │  accounts / permissions /
                    │  pointer-admin       │  quotas / releases
                    │  :8788               │
                    └───────┬──────────────┘
                            │ HTTPS
              ┌─────────────┴─────────────┐
              │                           │
    ┌─────────▼─────────┐       ┌─────────▼──────────┐
    │ client (desktop)  │       │ server             │  optional: shared
    │ managed flavour   │       │ pointer-server     │  model gateway
    └───────────────────┘       └────────────────────┘
```

| Role | What it does | Who runs it |
|---|---|---|
| **Control plane** | Login, permissions, quotas, audit, releases, download listing | IT / ops |
| **Client** | What employees use; takes identity from the control plane | IT builds once and distributes |
| **Server** | Shared model gateway for the team (optional) | IT / ops |

**The control plane is required; the server is optional.** The control plane alone is usable — each employee brings their own model API key.

---

## 2. Step one: deploy the control plane

<a id="control-plane"></a>

### 2.1 Prerequisites

- One Linux host (2 cores / 2 GB is plenty to start)
- A domain with a valid certificate (cookie auth and the OAuth callback both require it)
- A database: SQLite by default, PostgreSQL recommended in production

### 2.2 Environment variables

| Variable | Default | Notes |
|---|---|---|
| `POINTER_ADMIN_ADDR` | `127.0.0.1:8788` | Listen address. Keep it on loopback behind a reverse proxy; do not expose it directly |
| `POINTER_ADMIN_DB_URL` | `sqlite:pointer-admin.db` | Database connection string |
| `POINTER_ADMIN_STATIC_DIR` | none | Frontend build output; must point at `admin/web/dist` |
| `POINTER_ADMIN_DATA_DIR` | none | Runtime data (uploaded installers, model-catalog cache) |
| `POINTER_ADMIN_PUBLIC_BASE` | none | Public URL, e.g. `https://admin.example.com` |
| `POINTER_ADMIN_OAUTH_SECRET` | none | OAuth client secret, **required**, see §2.4 |
| `POINTER_ADMIN_SERVER_REDIRECT` | `http://127.0.0.1:8787/auth/callback` | Server callback; only change it when you deploy the server |
| `POINTER_ADMIN_TENANT` | default tenant | For multi-tenant deployments |
| `POINTER_ADMIN_COOKIE_SECURE` | see source | Must be true when serving over HTTPS |
| `POINTER_ADMIN_CAPTCHA` | see source | Can be disabled on an intranet; keep it on for public exposure |

Session lifetimes: `POINTER_ADMIN_SESSION_TTL`, `POINTER_ADMIN_ACCESS_TTL`, `POINTER_ADMIN_REFRESH_TTL`.

### 2.3 The bootstrap account

**The first start needs an account that can log in, or the service comes up and nobody can get in.** The startup log says so explicitly:

```
库里还没有任何账号。请设置 POINTER_ADMIN_BOOTSTRAP_USER 与
POINTER_ADMIN_BOOTSTRAP_PASSWORD 后重启，否则没人能登录。
```

```bash
POINTER_ADMIN_BOOTSTRAP_USER=admin \
POINTER_ADMIN_BOOTSTRAP_PASSWORD='use-a-strong-one' \
./pointer-admin
```

The first start creates the schema and seeds the default tenant automatically; there is no SQL to run by hand. Once the account exists you can drop both variables.

### 2.4 OAuth clients are registered automatically

When `POINTER_ADMIN_OAUTH_SECRET` is set, the control plane registers two clients on startup:

| Client | Kind | Redirect |
|---|---|---|
| Server | confidential | `POINTER_ADMIN_SERVER_REDIRECT` |
| Desktop | public (loopback) | none; uses a `127.0.0.1` loopback |

**Nothing to insert into the database by hand.** This secret must match the one baked into the client build, or the token exchange fails.

### 2.5 Reverse proxy and TLS

```nginx
location / {
    proxy_pass http://127.0.0.1:8788;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

`X-Forwarded-Proto` is required — the control plane uses it to decide whether the request is HTTPS, which decides whether the cookie gets `Secure`.

### 2.6 Build and package

The control plane does not build the frontend; it only serves whatever is in `POINTER_ADMIN_STATIC_DIR`. That means it ships as **two halves** — the binary and the frontend output — and the two must come from the same source.

One command does both:

```bash
npm run admin:build
# artifacts:
#   target/release/pointer-admin-bundle/pointer-admin-{os}-{arch}/
#   target/release/pointer-admin-bundle/pointer-admin-{os}-{arch}.tar.gz
```

The bundle contains `pointer-admin`, `web/`, `build-info.json` and `README.txt`. `build-info.json` records the commit and sha256 of each half — check it first when the binary looks new but the UI looks old.

Run it:

```bash
cd pointer-admin-{os}-{arch}
POINTER_ADMIN_STATIC_DIR=./web ./pointer-admin
```

To re-package without recompiling, use `npm run admin:package`. If either half is missing the script fails outright instead of producing a partial bundle — a control plane that starts but serves a blank page is far harder to diagnose than a build that refuses to finish.

### 2.7 Start, stop and maintain

**Production uses the shell script shipped inside the bundle** — no node, no npm:

```bash
cd pointer-admin-{os}-{arch}
./pointer-admin.sh start      # background start; writes a PID file
./pointer-admin.sh status     # exit 0 when running, 1 when stopped
./pointer-admin.sh restart
./pointer-admin.sh stop       # TERM first, KILL after 10s
```

The PID file and log live in the unpacked directory: `pointer-admin.pid`, `pointer-admin.log`.

The script points `POINTER_ADMIN_STATIC_DIR` at the sibling `web/` automatically, so you do not set it by hand. The database and data directory still come from environment variables — the script does not guess those, because they point at real state and a wrong guess is worse than a failed start.

**On a dev machine** use the npm scripts; equivalent, one node layer heavier:

```bash
npm run admin:start / admin:status / admin:stop / admin:restart
```

For boot-time start and crash restarts, hand it to systemd (`pointer-admin.sh` works as `ExecStart` with `Type=forking` + `PIDFile`, or point `ExecStart` at the binary with `Type=simple`).

---

<a id="client"></a>
## 3. Step two: build the client

The client is built from the open-source repo with the control-plane address baked into the binary.

### 3.1 Build

```bash
POINTER_EDITION=managed \
POINTER_API_BASE=https://admin.example.com \
npm run tauri:build
```

With `POINTER_EDITION=managed` but no control-plane address, `crates/pointer-core/build.rs` **panics** and the build fails. That is deliberate — it prevents shipping a half-bound artifact that looks like an enterprise build but talks to nobody.

### 3.2 Signing key pair

Automatic updates need a signing key pair:

```bash
# generate (keep the private key safe; never commit it)
npm run tauri signer generate -- -w ~/.pointer/updater.key
```

A `.pub` file is written next to the private key. **When a `.pub` sits beside the private key at build time, the build embeds that public key** — no edit to `src-tauri/tauri.conf.json` needed.

> Without the `.pub`, the build falls back to the repository's public key. Your signed packages then fail verification and **updates silently stop working** — no error, it just always says "up to date".

---

## 4. Step two: deploy the server

<a id="server"></a>

When you need this section: employees use a browser instead of the desktop client, or the team shares one model gateway. If neither applies, skip it — each employee enters their own model API key in the client.

Its artifact differs from the client:

```bash
npm run server:build
# target/release/pointer-server-bundle/pointer-server-{platform}-{arch}.zip
```

The server callback must line up with the control plane's `POINTER_ADMIN_SERVER_REDIRECT`, or the OAuth token exchange fails.

If you do not need a shared gateway, skip this section — employees enter their own model API keys in the client.

---

## 5. Step three: verify the wiring

Check these four things after deploying. **Actually observe each result**; do not infer from "the service started".

| # | What | How | Expected |
|---|---|---|---|
| 1 | Control plane is up | Open `https://admin.example.com/` | A login page, not a 404 or blank screen |
| 2 | Login works | Sign in with the §2.3 account | You land in the console with a full nav |
| 3 | Client knows the control plane | Open the built client → sign in | The login goes to `admin.example.com`, not some other domain |
| 4 | Links point at the right place | Click "Balance" in the client | It opens the billing page under `admin.example.com` |

Steps 3 and 4 matter most — they prove the baked address took effect. If you land on the vendor domain, `POINTER_API_BASE` did not apply at build time.

---

## 6. Automatic updates

The control plane serves `GET /api/updates/latest?target=…&arch=…&current_version=…`, and the client reads its update manifest from there.

Two prerequisites:

1. **Installers must be signed.** The manifest skips unsigned artifacts, so an unsigned upload does not count.
2. **The public key must match.** The key embedded in the client must pair with the private key that signed the package.

Upload from the releases page in the console; it requires `release:write`.

---

## 7. Model catalog

**A self-hosted control plane serves no platform model catalog.** After signing in, employees see an empty catalog and enter their own provider API keys.

This is by design: the open-source build ships no vendor credentials. Centralised model procurement is a private-extension concern, out of scope for this repository.

---

## 8. Go-live checklist

- [ ] Domain and certificate ready, `X-Forwarded-Proto` forwarded
- [ ] `POINTER_ADMIN_OAUTH_SECRET` set and identical to the client build's value
- [ ] `POINTER_ADMIN_BOOTSTRAP_USER` / `PASSWORD` set, account can log in
- [ ] `POINTER_ADMIN_STATIC_DIR` points at the frontend output, home page renders
- [ ] `POINTER_ADMIN_DATA_DIR` writable (installer uploads need it)
- [ ] Backup strategy for the database (single SQLite file / regular PG backups)
- [ ] Client build log shows `POINTER_EDITION=managed` with the domain baked
- [ ] A `.pub` sits next to the signing key, so the build embeds your own public key
- [ ] At least one **signed** installer uploaded
- [ ] Client login redirects to your control-plane domain
- [ ] Client "Balance" link points at your control plane
- [ ] Unneeded features disabled (captcha can be off on an intranet)

---

## 9. Troubleshooting

| Symptom | Check first |
|---|---|
| Blank home page / 404 | Is `POINTER_ADMIN_STATIC_DIR` correct, and did the frontend build run |
| Service is up but nobody can log in | Was the bootstrap account set; does the log contain the warning |
| Logged in, then immediately dropped | `POINTER_ADMIN_COOKIE_SECURE` versus HTTPS; is the proxy forwarding `X-Forwarded-Proto` |
| Client login goes to the vendor domain | `POINTER_API_BASE` did not apply at build time; check the build log |
| Always says "up to date" | Signing public key mismatch, or the installer is unsigned |
| Client reports a token-exchange failure | The control plane's `OAUTH_SECRET` does not match the client's |

---

## Related

- [Packaging and Deployment](README.md) — flavour and runtime form overview
- [Build artifacts and flavours](editions.md) — how to build each artifact
- [Platform packaging](platforms.md) — differences across the three platforms
- [Desktop OAuth integration](../developer/desktop-oauth-web-integration.md) — token exchange details
