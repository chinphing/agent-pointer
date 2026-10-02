# Docs site rules

English | [简体中文](../../zh-CN/contributing/docs-site.md)

`docs/` is the site source and `docs-site/` is the VitePress project. **There is exactly one source of truth for the publishing rules**: `docs-site/site-map.mjs`.

## The rule in one sentence

**A blacklist**: any page under `docs/**/*.md` is **published automatically as long as it is not in `SITE_EXCLUDES`**. New documentation does not need to be "registered" to go live — but you must add the **sidebar order** and the **index page** yourself (see "Adding a new document").

## Three tiers

| Tier | Directory | On site | Sidebar position |
|----|------|------|-----------|
| tier 1 | `user/` `developer/` `contributing/` `deploy/` | ✅ | each gets its own top-level section |
| tier 2 | `internals/` `agents/` `llm/` `ui/` | ✅ | merged into the "Internals" section, each a collapsible group, with a **maintainer-notes banner** added automatically |
| tier 3 | `design/` `plans/` `settings-refactor/` `superpowers/` | ❌ | not on the site |

> "tier" is only a convention in the comments of `site-map.mjs`; **there is no tier constant in the code**. What actually takes effect is `SITE_EXCLUDES` below.

## The exclusion list

```js
// docs-site/site-map.mjs
export const SITE_EXCLUDES = [
  'README.md', // docs/ language picker — collides with the zh-CN index route
  'zh-CN/design',
  'zh-CN/plans',
  'zh-CN/settings-refactor',
  'zh-CN/superpowers',
  'en/design',
];
```

- **Matching rule**: exact equality, or a directory starting with that prefix (`p === ex || p.startsWith(ex + '/')`) — so writing a directory name excludes the whole directory
- `SRC_EXCLUDE_GLOBS` is its **VitePress mirror** (fed to `srcExclude`); changing one side requires changing the other as well
- Relative links pointing at pages that are not on the site are rewritten into **GitHub blob links** at build time (`doc-links.mts`) and never become dead links

## Language and routing

| Fact | Value |
|------|-----|
| **Default language** | **English** — the `en/` prefix of `docs/en/**` is stripped and routes land at the root (`docs/en/user/skills.md` → `/user/skills`) |
| Chinese | keeps the `zh-CN/` prefix (→ `/zh-CN/user/skills`) |
| Directory index | each directory's `README.md` → that directory's index route (`/zh-CN/user/`) |
| URL shape | `cleanUrls: true` → URLs **have no `.html`**; the artifacts are still `foo.html` / `index.html` |
| base | `/agent-pointer/` (GitHub Pages project site, derived from `GITHUB_REPO`) |

## Where the sidebar comes from

`sidebar.mts` generates the sidebar in reverse from the **published set**:

- New documents **appear automatically** in the sidebar, with no config change
- Pages that are not on the site **never** enter the sidebar and never become dead links
- The title comes from the file's **first H1** (skipping code blocks), falling back to the file name when there is no H1

Sections are declared in `config.mts`. Chinese has 6: 快速开始 / 使用指南 / **开发** / 部署 / 参与贡献 / 维护者笔记; English also has 6 (Getting Started / Guides / Development / Deployment / Contributing / Internals), just with less content.

A section can be assembled in three ways:

| Field | Meaning |
|------|------|
| `files` | a hand-picked list of pages, **in the given order** (e.g. the "Development → Concepts" group lists 6 developer pages) |
| `dirs` | second-level grouping. It can be a whole-directory listing (`dir` + `exclude`, where `exclude` uses `docs/`-relative paths, so it can pull pages from **other directories**), a hand-picked list (only `files`, no `dir`), or both; it renders as a collapsible group only when `label` is present |
| `items` | hand-written entries (rarely used) |

**Ordering rule** (`PRIORITY` in `sidebar.mts`): entries listed in the array sort by array index; the rest come after, sorted by file name with `localeCompare`.

### Reassigning a section means changing the config, not the path

A page's physical location and its sidebar membership **can differ**. Example: `contributing/macos-window-chrome.md` and `contributing/web-media-and-desktop-snapshot.md` are about UI implementation, so the sidebar files them under "Internals → UI notes":

```ts
// config.mts
{ text: '参与贡献', dirs: [{ dir: 'zh-CN/contributing', exclude: [ /* 两篇 UI 文档 */ ] }] },
{ text: '维护者笔记', dirs: [ /* … */, { dir: 'zh-CN/ui', label: 'UI 笔记', files: [ /* 两篇 UI 文档 */ ] }] },
```

> **Why not move the files**: moving changes the URL and breaks every inbound link (these two have 6 inbound links, one of them a source comment in `Composer.vue`); the only gain is a tidier directory. So the convention is **reassign via config, not by moving paths**.

⚠️ One trap: **the maintainer-notes banner is injected by path** (the regex in `config.mts` matching `internals|agents|llm|ui`). When you "reassign" a page into the Internals section, remember to add its path to that regex too, otherwise you get "in the Internals section but without the banner".

### Example: the four groups of the "Development" section

The 33 pages in `zh-CN/developer/` are split by topic into 4 groups plus a catch-all, with the page lists in the `ZH_DEV_*` constants of `config.mts`:

| Group | Pages | Content |
|----|:---:|------|
| **Concepts** | 6 | architecture, workspace root, attachment storage, extension hooks, Skills compatibility and persistence |
| **Tools and protocols** | 10 | tool-calling protocol, `run_subagent`, file / web / terminal / session tools, logging |
| **Integrations** | 5 | IM channels, cloud hosts, desktop OAuth, MCP, Webhook |
| **Troubleshooting** | 8 | streaming errors and reconnect, auth refresh, text truncation, chunking, session id, turn baseline, message position collisions |
| **Other** | catch-all | `cli.md`, `standalone-local-login.md`, and **new pages not listed in the four groups above** |

The catch-all is generated with `dir` + `exclude` — when you add a developer document, even if you forget to file it, it still shows up under "Other" and **never drops out of the sidebar**. `standalone-deployment.md` is the exception: it hangs under the "Deployment" section.

The first item of the section is [`developer/README.md`](../developer/README.md) (the index page, specified explicitly with `files`).

## Adding a new document

1. Write `docs/zh-CN/<section>/<name>.md` — **it is published automatically unless it is in `SITE_EXCLUDES`**
2. To control the sidebar position → add it to `PRIORITY['zh-CN/<section>']` in `sidebar.mts`
3. Add it to the index table in that directory's `README.md`
4. `node scripts/check-doc-links.mjs` → 0 broken links
5. `npm run docs:build` to build once locally
6. To see the result → preview (see below)

## Local preview and build

```bash
npm run docs:install   # first time: npm --prefix docs-site install
npm run docs:dev       # dev server (default 5173, hot reload)
npm run docs:build     # build → docs-site/.vitepress/dist
npm run docs:preview   # preview the build output (default 4173)
```

### ⚠️ Do not pass a path argument to preview

```bash
cd docs-site && npx vitepress preview docs-site   # ❌ ENOENT
cd docs-site && npx vitepress preview             # ✅
```

The original error:

```
failed to start server. error:
ENOENT: no such file or directory, open '…/docs-site/docs-site/.vitepress/dist/404.html'
```

Cause: **positional arguments to `vitepress preview` are treated as the VitePress root**, and the artifact directory is computed as `<root>/.vitepress/dist` — passing an extra `docs-site` nests one level deeper, so `404.html` cannot be found.

- Running inside `docs-site/` → do **not** pass a path
- Running at the repository root → use `npm run docs:preview` (the script switches to `docs-site/`)

## Link validation

```bash
node scripts/check-doc-links.mjs
```

- **Zero dependencies**: CI installs nothing and just runs `node`
- Scans **all** Markdown (**including the tier 3 pages that are not on the site**), checking relative links and anchors, skipping code blocks and inline code
- Resolves `/route`-style links against the published set from `site-map.mjs` — route links pointing at pages that are not on the site **count as broken**
- Broken links exit with code `1`; a usage error exits with `2`

In CI it runs from `docs-links.yml`; building and publishing run from `docs-site.yml`, see [CI and release flow](ci.md).

## Related

- [CI and release flow](ci.md) — triggers and commands of `docs-links.yml` / `docs-site.yml`
- [Cross-platform development and packaging](cross-platform-build.md) — platform environment preparation
- [Command-line and script reference](../developer/cli.md) — a walkthrough of the `docs:*` scripts
- Site project documentation: [`docs-site/README.md`](../../../docs-site/README.md)
