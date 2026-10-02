# Docs site (`docs-site/`)

VitePress project that publishes the existing [`docs/`](../docs) tree to GitHub
Pages at **https://chinphing.github.io/agent-pointer/**.

Nothing under `docs/` is moved, renamed or rewritten. The site mounts that tree
in place through `srcDir` + `rewrites`:

| Concern | Where |
| --- | --- |
| Route map, excludes, GitHub URL helpers | `site-map.mjs` |
| Site config (i18n, sidebar, link rewriting) | `.vitepress/config.mts` |
| Markdown-it plugins (link resolution, stray HTML) | `.vitepress/doc-links.mts` |
| Sidebar generation | `.vitepress/sidebar.mts` |
| Theme extension (banner, brand tokens) | `.vitepress/theme/` |

## Commands

Run from the repository root:

```bash
npm run docs:install   # npm --prefix docs-site install
npm run docs:dev       # local dev server with hot reload
npm run docs:build     # production build → docs-site/.vitepress/dist
npm run docs:preview   # serve the built output
```

## How the tree is mounted

- **Source dir** — `docs/` itself; `en/` is stripped so **English is the default
  locale at `/`** and Chinese stays under `/zh-CN/` (`DEFAULT_LOCALE` in
  `site-map.mjs`).
- **`README.md` → `index.md`** — every directory README becomes that
  directory's index route (`docs/zh-CN/user/README.md` → `/zh-CN/user/`,
  `docs/en/user/README.md` → `/user/`).
- **Off-site content** — `docs/README.md` (the language picker) plus
  `zh-CN/design/`, `zh-CN/plans/`, `zh-CN/settings-refactor/`,
  `zh-CN/superpowers/` and `en/design/` are excluded via `srcExclude`; links
  pointing at them are rewritten to GitHub blob URLs at build time instead of
  becoming dead links.
- **Stray HTML** — prose placeholders such as `<plugin name>` or `<String>`
  would break the Vue SFC template; `.vitepress/doc-links.mts` turns unknown
  tags back into literal text.

`scripts/check-doc-links.mjs` reads the same `site-map.mjs`, so CI validates
`/route` links against the published page set.

## Deployment

`.github/workflows/docs-site.yml` builds the site and publishes it with
`actions/upload-pages-artifact` + `actions/deploy-pages`. GitHub Pages must be
switched to the **GitHub Actions** source in *Settings → Pages* once.
