/**
 * Single source of truth for how the existing `docs/` tree is mounted onto the
 * VitePress site. Zero dependencies on purpose — `scripts/check-doc-links.mjs`
 * runs it in CI with no install step.
 *
 * Consumed by:
 *   - `docs-site/.vitepress/config.mts` — srcDir, rewrites, link rewriting
 *   - `scripts/check-doc-links.mjs`     — site-route aware link checking
 *
 * Rules (see the task's "three tiers" of content):
 *   tier 1  user / developer / contributing / deploy  → first-class sidebar
 *   tier 2  internals / agents / llm / ui             → sidebar, "maintainer notes"
 *   tier 3  design / plans / settings-refactor / superpowers → NOT published
 *
 * Nothing under `docs/` is moved or edited: the site mounts the tree in place
 * through `srcDir` + `rewrites`, and links that leave the published set are
 * rewritten to GitHub at build time.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** `docs-site/` — the VitePress project root. */
export const SITE_DIR = path.dirname(fileURLToPath(import.meta.url));
/** Repository root (`agent-pointer/`). */
export const REPO_ROOT = path.resolve(SITE_DIR, '..');
/** `docs/` — the VitePress `srcDir`, mounted in place. */
export const SRC_DIR = path.join(REPO_ROOT, 'docs');

/**
 * Project site on GitHub Pages: `https://<owner>.github.io/<repo>/`.
 * Owner/repo measured from `git remote -v` (`chinphing/agent-pointer`), not guessed.
 */
export const GITHUB_OWNER = 'chinphing';
export const GITHUB_REPO = 'agent-pointer';
export const GITHUB_BRANCH = 'main';
export const SITE_BASE = `/${GITHUB_REPO}/`;
export const GITHUB_BLOB_URL = `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/blob/${GITHUB_BRANCH}`;

/**
 * Locale key → source subtree. `root` is the default (English) locale, served
 * at `/`; Chinese keeps its `zh-CN/` prefix and is served at `/zh-CN/`.
 */
export const LOCALES = { root: 'en', 'zh-CN': 'zh-CN' };

/** Source subtree of the default locale — stripped from the published route. */
export const DEFAULT_LOCALE = 'en';
/** Source subtree of the secondary locale — kept as a route prefix. */
export const SECONDARY_LOCALE = 'zh-CN';

/**
 * `docs/` sub-trees that are deliberately kept off the site.
 * Kept in sync with VitePress `srcExclude` (globs are srcDir-relative).
 */
export const SITE_EXCLUDES = [
  'README.md', // docs/ language picker — collides with the zh-CN index route
  'zh-CN/design',
  'zh-CN/plans',
  'zh-CN/settings-refactor',
  'zh-CN/superpowers',
  'en/design',
];

/** VitePress `srcExclude` globs for the same set. */
export const SRC_EXCLUDE_GLOBS = [
  // NOTE: no leading `/` — VitePress' glob treats `/x` as an absolute pattern and
  // walks the whole filesystem. A slash-less pattern matches the top-level file only.
  'README.md',
  'zh-CN/design/**',
  'zh-CN/plans/**',
  'zh-CN/settings-refactor/**',
  'zh-CN/superpowers/**',
  'en/design/**',
];

const posix = (p) => p.split(path.sep).join('/');

/** Is a `docs/`-relative markdown path published on the site? */
export function isOnSite(rel) {
  const p = posix(rel);
  if (!p.endsWith('.md')) return false;
  return !SITE_EXCLUDES.some((ex) => p === ex || p.startsWith(`${ex}/`));
}

/**
 * Published file path for a `docs/`-relative markdown path.
 * - strips the `en/` prefix (English is the default locale, served at `/`)
 * - every `README.md` becomes `index.md`, so directory routes end in `/`
 * The `zh-CN/` prefix is kept (Chinese is served under `/zh-CN/`).
 */
export function rewritePage(rel) {
  let out = posix(rel);
  if (out.startsWith(`${DEFAULT_LOCALE}/`)) out = out.slice(DEFAULT_LOCALE.length + 1);
  if (out.endsWith('README.md')) out = `${out.slice(0, -'README.md'.length)}index.md`;
  return out;
}

/** Site route (without `base`) for a `docs/`-relative markdown path. */
export function pageRoute(rel) {
  const r = rewritePage(rel);
  if (r === 'index.md') return '/';
  if (r.endsWith('/index.md')) return `/${r.slice(0, -'index.md'.length)}`;
  return `/${r.slice(0, -'.md'.length)}`;
}

/** GitHub blob URL for a repository-relative path. */
export function githubBlobUrl(repoRel, suffix = '') {
  return `${GITHUB_BLOB_URL}/${posix(repoRel)}${suffix}`;
}

/** Every published page, indexed by route and by `docs/`-relative path. */
export function buildSiteIndex({ srcDir = SRC_DIR } = {}) {
  /** @type {Map<string, string>} route → docs-relative file */
  const byRoute = new Map();
  /** @type {Set<string>} docs-relative files */
  const rels = new Set();
  const walk = (dir) => {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const abs = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(abs);
      } else if (entry.isFile() && entry.name.toLowerCase().endsWith('.md')) {
        const rel = posix(path.relative(srcDir, abs));
        if (!isOnSite(rel)) continue;
        rels.add(rel);
        byRoute.set(pageRoute(rel), rel);
      }
    }
  };
  walk(srcDir);
  return { byRoute, rels };
}

/**
 * Inverse of `rewritePage` for a published page: the `docs/`-relative source
 * path that produced it (`user/skills.md` → `en/user/skills.md`,
 * `zh-CN/user/index.md` → `zh-CN/user/README.md`).
 */
export function sourcePath(pagePath) {
  let p = posix(pagePath);
  if (!p.startsWith(`${SECONDARY_LOCALE}/`)) p = `${DEFAULT_LOCALE}/${p}`;
  if (p.endsWith('index.md')) p = `${p.slice(0, -'index.md'.length)}README.md`;
  return p;
}

/** The other locale's path for a `docs/`-relative source path, or `null`. */
export function counterpartSource(rel) {
  const p = posix(rel);
  const pairs = [
    [DEFAULT_LOCALE, SECONDARY_LOCALE],
    [SECONDARY_LOCALE, DEFAULT_LOCALE],
  ];
  for (const [from, to] of pairs) {
    if (p.startsWith(`${from}/`)) return `${to}/${p.slice(from.length + 1)}`;
  }
  return null;
}

/** Route of each locale's landing page (without `base`). */
const LOCALE_HOME = { [DEFAULT_LOCALE]: '/', [SECONDARY_LOCALE]: '/zh-CN/' };

/**
 * Language-switch target for a published page.
 *
 * `pagePath` is the rewritten path VitePress reports in `pageData.relativePath`.
 * Returns `{ text, link }` pointing at the *other* locale: the translated
 * counterpart when it is published, otherwise that locale's landing page.
 * The fallback matters — English only covers a subset of the docs, so a
 * corresponding-page link would dead-link on most Chinese pages (and on
 * `DEVELOPMENT.md`, which has no Chinese counterpart).
 */
export function alternateLocale(pagePath, index = buildSiteIndex()) {
  const src = sourcePath(pagePath);
  if (!index.rels.has(src)) return null; // not a published page (e.g. the 404 shell)
  const other = counterpartSource(src);
  if (!other) return null;
  const secondary = other.startsWith(`${SECONDARY_LOCALE}/`);
  const text = secondary ? '简体中文' : 'English';
  const home = LOCALE_HOME[secondary ? SECONDARY_LOCALE : DEFAULT_LOCALE];
  return { text, link: index.rels.has(other) ? pageRoute(other) : home };
}

/** `#fragment` + `?query` tail of a link, kept verbatim. */
export function linkSuffix(href) {
  const i = href.search(/[?#]/);
  return i === -1 ? '' : href.slice(i);
}

/** Link target without its `?query` / `#fragment` tail. */
export function linkPath(href) {
  const i = href.search(/[?#]/);
  return i === -1 ? href : href.slice(0, i);
}

/** Anything VitePress treats as external: `scheme:` or protocol-relative. */
export function isExternalHref(href) {
  return /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(href);
}
