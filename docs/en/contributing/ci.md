# CI and release flow

English | [简体中文](../../zh-CN/contributing/ci.md)

The repository has **4 workflows** (`.github/workflows/`). This page explains what each one does, how to reproduce its checks locally, and what to run before opening a PR.

```
ci.yml          ← main CI: tests + audit + manifest consistency + secret scanning (runs on PRs and main)
docs-links.yml  ← documentation relative-link and anchor validation
docs-site.yml   ← build the docs site and publish it to GitHub Pages
release.yml     ← after a tag, package the desktop client for the three platforms (standalone only)
```

## The four workflows at a glance

| File | Name | Trigger | Purpose |
|------|------|------|------|
| `ci.yml` | CI | PR, push to `main`, manual | Frontend and Rust tests, dependency audit, third-party manifest, gitleaks |
| `docs-links.yml` | Docs links | PR, push to `main` | `node scripts/check-doc-links.mjs` |
| `docs-site.yml` | Docs site | push to `main` (changes under `docs/**` / `docs-site/**`), manual | Build the docs site → GitHub Pages |
| `release.yml` | Release (standalone) | push tag `v*.*.*`, manual | Package the desktop client for the three platforms → Draft Release |

## `ci.yml`: the only main CI

A single job (`test`) runs on `ubuntu-24.04`, with **no** `concurrency` and no `timeout-minutes`.

| Step | Command |
|------|------|
| Checkout | `actions/checkout@v4` |
| Free disk space | `sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc …`, `docker system prune -af` |
| Node | `actions/setup-node@v4`, **`node-version: 20`** |
| Rust | `dtolnay/rust-toolchain@stable` |
| Install Linux system dependencies | `apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev …` |
| Install frontend dependencies | **`npm ci`** |
| Frontend tests | `npm test` |
| Rust tests | `cargo test --workspace` |
| Rust dependency audit | `cargo install cargo-audit --locked` + `cargo audit` |
| npm dependency audit | `npm audit --audit-level=high` |
| Third-party manifest consistency | `npm run licenses` + `git diff --exit-code -- THIRD-PARTY-NOTICES.md` |
| Secret scanning | `gitleaks/gitleaks-action@v2` |

> "Free disk space" is a CI-runner-only step, **do not run it on your own machine** — it deletes system directories.

### Reproducing locally

```bash
npm ci
npm test
cargo test --workspace
cargo install cargo-audit --locked   # needed the first time
cargo audit
npm audit --audit-level=high
npm run licenses && git diff --exit-code -- THIRD-PARTY-NOTICES.md
```

Three easy traps:

1. **`THIRD-PARTY-NOTICES.md` must match the dependencies** — after changing dependencies run `npm run licenses` and commit the regenerated manifest as well; otherwise CI fails at `git diff --exit-code`.
2. **`cargo audit` reads `.cargo/audit.toml`**, which holds two exceptions (the quick-xml / ossify chain), marked `REVISIT BY: 2027-01-01`. When a newly added dependency is blocked by the audit, check here first.
3. **The Node version is 20**, hard-coded in the workflow (the repository has no `.nvmrc`). If your local Node version differs, align it before debugging.

gitleaks uses the official action with the default checkout depth; to check locally, install `gitleaks` and run `gitleaks detect`.

## `docs-links.yml`

**Zero dependencies**: no npm packages installed, no build, just

```bash
node scripts/check-doc-links.mjs
```

It checks the **relative links and anchors** of every Markdown file in the repository (skipping code blocks and inline code) and resolves `/route`-style links against the **published page set** in `docs-site/site-map.mjs` — so a route link pointing at a page that is not on the site is counted as a broken link. For the detailed rules see [Docs site rules](docs-site.md).

## `docs-site.yml`

- **Trigger**: push to `main` with changes under `docs/**`, `docs-site/**` or this workflow itself; manual runs are supported too
- **Permissions**: `contents: read` + `pages: write` + `id-token: write` (OIDC, no repo secret needed)
- **Concurrency**: `group: pages`, does not cancel an in-progress deployment
- **Steps**:

```bash
npm ci                      # working-directory: docs-site
npm run docs:build          # run at the repository root → npm --prefix docs-site run build
# upload docs-site/.vitepress/dist → actions/deploy-pages
```

> The first time you enable it, pick **GitHub Actions** as the Source under *Settings → Pages* in the GitHub repository.

## `release.yml`

**It only produces the standalone client**: all three platforms pass `--config src-tauri/tauri.personal.conf.json`, no control-plane domain is injected, and there are no updater artifacts. managed (official / enterprise) packages must be built locally or in enterprise CI, see [editions.md](../deploy/editions.md).

| Runner | Extra arguments | Artifacts |
|--------|----------|------|
| `windows-latest` | — | Windows MSI |
| `macos-latest` | `--target universal-apple-darwin` | Universal macOS |
| `ubuntu-24.04` | — | Linux deb + AppImage |

Steps: checkout → Node 20 → Rust stable (two extra targets on macOS) → (install system dependencies on Linux) → **`npm install`** → `npm run icons` → `tauri-apps/tauri-action` → create a **Draft** Release.

- ⚠️ This uses `npm install` rather than `npm ci` (unlike `ci.yml`) — do not copy it elsewhere
- The Release is a **draft** and needs a human to confirm before publishing
- macOS signing secrets: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`, `APPLE_ID`, `APPLE_PASSWORD` (injected on the macOS branch only)
- For per-platform environment preparation and artifact details see [Cross-platform development and packaging](cross-platform-build.md)

## Versioning: a manual gate, CI does not catch it

**No workflow contains a `VERSION`, `version:sync` or `version:check` step.** The version is a manual process:

```bash
# 1) edit only the root VERSION
# 2) sync it to the literals elsewhere
npm run version:sync
# 3) confirm there is no drift (exit code 1 on drift)
npm run version:check
# 4) commit, then tag to trigger release.yml
git tag v0.1.3 && git push origin v0.1.3
```

> ⚠️ **`version:check` is not wired into CI.** If you forget to run `version:sync`, CI will not complain; only a local `version:check` (or a human review) can spot the drift. Likewise, the release workflow **does not verify that the tag matches `VERSION`**.
>
> For which files the sync writes to and where the derived values are used, see [Version management](versioning.md).

## Pre-PR checklist

- [ ] `npm test` — frontend unit tests
- [ ] `cargo test --workspace` (or only the crate you changed)
- [ ] Changed dependencies → `npm run licenses`, and commit `THIRD-PARTY-NOTICES.md` too
- [ ] Changed docs → `node scripts/check-doc-links.mjs` (0 broken links)
- [ ] Changed `VERSION` → `npm run version:sync` + `npm run version:check`
- [ ] Commit with `Signed-off-by` (use `git commit -s`)
- [ ] In the PR description tick the Test plan in the template (`npm test` / `cargo test` / whether the bound and unbound defaults were checked)

## DCO: sign-off provenance (a convention, not CI-enforced)

`CONTRIBUTING.md` requires every commit to carry:

```
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` adds this line automatically. Signing off means you confirm that the contribution is yours to submit and can be licensed under Apache-2.0.

**But nothing in the repository enforces it** — there is no DCO workflow; `.github/` only mentions it in one line of the PR template. In other words, a missing sign-off will not be caught by CI, only by review.

## Dependabot

`.github/dependabot.yml` declares only two ecosystems: `npm` and `cargo`, `directory: /`, `schedule.interval: weekly`, and both set

```yaml
open-pull-requests-limit: 0
```

The meaning is that **regular version-upgrade PRs are all disabled** (too noisy, and major-version jumps tend to turn CI red), while **security updates are not subject to this limit** and are still opened by Dependabot security updates. So a Dependabot PR is almost always a security update — handle it with priority.

## Related

- [Cross-platform development and packaging](cross-platform-build.md) — environments, build artifacts and release details for the three platforms
- [Version management](versioning.md) — `VERSION` as the single source and the places it is synced to
- [Docs site rules](docs-site.md) — publishing rules and link validation
- [Command-line and script reference](../developer/cli.md) — a walkthrough of the `npm run` scripts used above
