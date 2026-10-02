#!/usr/bin/env node
/**
 * Relative markdown link checker for the whole repo (file existence + anchors).
 *
 * Zero dependencies: CI runs `node scripts/check-doc-links.mjs` with no install step.
 *
 * Usage:
 *   node scripts/check-doc-links.mjs [--verbose] [--include-code-fences] [--root <dir>]
 *
 * Scope: relative links only. Absolute URLs (`http://`, `https://`, `mailto:`, any
 * other scheme) and protocol-relative `//host/...` are not checked. A `#fragment`
 * is resolved against the target file's heading ids (github-slugger rules) and
 * explicit `<a name|id>` / `{#custom-id}` anchors.
 *
 * Fenced code blocks and inline code are literal text, so links inside them are
 * NOT links (GitHub / lychee behaviour) — this is the default. It matters:
 * THIRD-PARTY-NOTICES.md embeds upstream license prose that contains
 * `[LICENSE-APACHE](LICENSE-APACHE)` inside a fence; naive mode reports it as broken.
 * `--include-code-fences` restores that naive mode for debugging.
 *
 * Site awareness: `docs/` is mounted in place by the VitePress site in
 * `docs-site/` (route map in `docs-site/site-map.mjs`). Site routes do not match
 * file paths (`en/x.md` → `/x`, `zh-CN/x.md` → `/zh-CN/x`, `README.md` → `/index`), so this script also:
 *   - classifies every relative link as `site` (stays on the site) or `github`
 *     (rewritten to a GitHub blob URL at build time), and
 *   - resolves `/route` links against the published page set, reporting
 *     `missing-route` — the failure mode that would 404 the built site.
 * `/route` links are only read as site routes while the scanned root contains
 * the docs tree, so `--root <tmpdir>` keeps the plain filesystem reading.
 *
 * Exit code: 1 when any link is broken, 0 otherwise (2 on usage error).
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { REPO_ROOT, SRC_DIR, buildSiteIndex, isOnSite } from '../docs-site/site-map.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = path.resolve(SCRIPT_DIR, '..');

/** Directories that never hold authored markdown. */
const SKIP_DIRS = new Set([
  'node_modules', '.git', 'target', 'dist', 'build', 'out',
  'coverage', '.vite', '.next', '.cache', 'vendor',
]);

const MD_EXT = new Set(['.md', '.mdx']);

/** `[text](target "title")` — the target is the only capture group. */
const LINK_RE = /!?\[[^\]]*\]\(\s*(<[^>]*>|[^)\s]+)(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)/g;

const HEADING_RE = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;

/** Blank out fenced code blocks and inline code so their text is not parsed as links. */
export function maskCode(text) {
  const out = [];
  let fence = null;
  for (const line of text.split('\n')) {
    const m = /^\s{0,3}(`{3,}|~{3,})/.exec(line);
    if (fence) {
      if (m && m[1][0] === fence[0] && m[1].length >= fence.length) fence = null;
      out.push(' '.repeat(line.length));
      continue;
    }
    if (m) {
      fence = m[1];
      out.push(' '.repeat(line.length));
      continue;
    }
    out.push(line.replace(/`+[^`]*`+/g, (s) => ' '.repeat(s.length)));
  }
  return out.join('\n');
}

/**
 * Heading text -> anchor id, following github-slugger:
 * lowercase, drop inline markdown/HTML, strip punctuation/symbols/control chars
 * (keeping ASCII `-` and `_`), every space becomes `-` (no collapsing).
 */
export function slugify(heading) {
  let s = heading.trim();
  s = s.replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1');
  s = s.replace(/<\/?[^>]+>/g, '');
  s = s.toLowerCase();
  s = s.replace(/[\p{P}\p{S}\p{C}]/gu, (c) => (c === '-' || c === '_' ? c : ''));
  s = s.replace(/[\p{Zs}\p{Zl}\p{Zp}]/gu, ' ');
  return s.replace(/ /g, '-');
}

/** All anchor ids a markdown file exposes: headings (deduped `-1`/`-2`), `<a name|id>`, `{#id}`. */
export function anchorsOf(file) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return null;
  }
  const anchors = new Set();
  const seen = new Map();
  for (const line of maskCode(text).split('\n')) {
    const heading = HEADING_RE.exec(line);
    if (heading) {
      const base = slugify(heading[2]);
      const n = seen.get(base) ?? 0;
      seen.set(base, n + 1);
      anchors.add(n === 0 ? base : `${base}-${n}`);
    }
    for (const m of line.matchAll(/<a\s+[^>]*(?:name|id)=["']([^"']+)["']/gi)) anchors.add(m[1]);
    for (const m of line.matchAll(/\{#[^}\s]+\}/g)) anchors.add(m[0].slice(2, -1));
  }
  return anchors;
}

/** Every markdown file under `root`, skipping generated/vendored directories. */
export function walkMarkdown(dir, out = []) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      walkMarkdown(path.join(dir, entry.name), out);
    } else if (entry.isFile() && MD_EXT.has(path.extname(entry.name).toLowerCase())) {
      out.push(path.join(dir, entry.name));
    }
  }
  return out;
}

/** `/user/` and `/user` are the same site route; the root stays `/`. */
function normalizeRoute(route) {
  const trimmed = route.replace(/\/+$/, '');
  return trimmed === '' ? '/' : trimmed;
}

/**
 * Site context for a scanned root, or `null` when the root does not contain the
 * VitePress source tree (e.g. the `--root <tmpdir>` unit tests) — in that case
 * `/route` links keep the plain filesystem reading.
 */
export function resolveSiteContext(root) {
  const rel = path.relative(root, SRC_DIR);
  if (rel.startsWith('..') || path.isAbsolute(rel)) return null;
  const { byRoute } = buildSiteIndex();
  const routes = new Map();
  for (const [route, file] of byRoute) routes.set(normalizeRoute(route), file);
  return { srcDir: SRC_DIR, repoRoot: REPO_ROOT, routes };
}

/**
 * Check every relative link in every markdown file under `root`.
 * Returns `{ files, links, broken }`; `links` holds one record per checked link.
 */
export function findBrokenLinks({
  root = DEFAULT_ROOT,
  includeCodeFences = false,
  site = resolveSiteContext(root),
} = {}) {
  const files = walkMarkdown(root).sort();
  const anchorCache = new Map();
  const links = [];
  const broken = [];

  for (const file of files) {
    const raw = fs.readFileSync(file, 'utf8');
    const text = includeCodeFences ? raw : maskCode(raw);
    const rel = path.relative(root, file);

    text.split('\n').forEach((line, i) => {
      for (const match of line.matchAll(LINK_RE)) {
        let target = match[1];
        if (target.startsWith('<') && target.endsWith('>')) target = target.slice(1, -1);
        if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(target)) continue; // http:, mailto:, …
        if (target.startsWith('//')) continue;

        const [rawPath, fragment] = target.split('#');
        const decodedPath = decodeURIComponent(rawPath ?? '');
        const record = { file: rel, line: i + 1, target, resolved: null, kind: null, scope: 'local' };

        /** Anchor ids of `targetFile`, cached for the whole scan. */
        const checkAnchor = (targetFile) => {
          if (!fragment) return;
          if (!anchorCache.has(targetFile)) anchorCache.set(targetFile, anchorsOf(targetFile));
          const anchors = anchorCache.get(targetFile);
          if (anchors && !anchors.has(decodeURIComponent(fragment))) record.kind = 'missing-anchor';
        };

        // A `/route` link addresses the built site, not the filesystem.
        if (decodedPath.startsWith('/') && site) {
          record.scope = 'route';
          const page = site.routes.get(normalizeRoute(decodedPath));
          if (page) {
            record.resolved = page;
            checkAnchor(path.join(site.srcDir, page));
          } else {
            record.resolved = decodedPath;
            record.kind = 'missing-route';
          }
          links.push(record);
          if (record.kind) broken.push(record);
          continue;
        }

        let resolved;
        if (!decodedPath) resolved = file;
        else if (decodedPath.startsWith('/')) resolved = path.join(root, decodedPath.slice(1));
        else resolved = path.resolve(path.dirname(file), decodedPath);
        record.resolved = path.relative(root, resolved);

        if (site) {
          const siteRel = path.relative(site.srcDir, resolved).split(path.sep).join('/');
          if (!siteRel.startsWith('..') && isOnSite(siteRel)) {
            record.scope = 'site'; // stays on the site (build emits a site route)
          } else {
            const repoRel = path.relative(site.repoRoot, resolved).split(path.sep).join('/');
            // published nowhere: the build rewrites it to a GitHub blob URL
            if (!repoRel.startsWith('..') && !path.isAbsolute(repoRel)) record.scope = 'github';
          }
        }

        if (!fs.existsSync(resolved)) {
          record.kind = 'missing-path';
        } else if (
          fragment &&
          fs.statSync(resolved).isFile() &&
          MD_EXT.has(path.extname(resolved).toLowerCase())
        ) {
          checkAnchor(resolved);
        }

        links.push(record);
        if (record.kind) broken.push(record);
      }
    });
  }

  return { files, links, broken, site };
}

const HELP = `Usage: node scripts/check-doc-links.mjs [options]

Checks relative links in every markdown file: target file exists, and #fragment
matches an anchor in that file. Fenced code blocks and inline code are skipped.

When the scanned root contains the VitePress source tree (docs/), links are also
read through the site route map: /route links must match a published page, and
each relative link is classified as "site" (stays on the site) or "github"
(rewritten to a GitHub blob URL at build time).

Options:
  --verbose                list every checked link, not just the broken ones
  --include-code-fences    naive mode: also scan inside fenced code blocks
  --root <dir>             scan <dir> instead of the repository root
  -h, --help               show this help
`;

function parseArgs(argv) {
  const opts = { verbose: false, includeCodeFences: false, root: DEFAULT_ROOT, help: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--verbose') opts.verbose = true;
    else if (arg === '--include-code-fences') opts.includeCodeFences = true;
    else if (arg === '--root') opts.root = path.resolve(argv[(i += 1)] ?? '');
    else if (arg.startsWith('--root=')) opts.root = path.resolve(arg.slice('--root='.length));
    else if (arg === '-h' || arg === '--help') opts.help = true;
    else throw new Error(`unknown argument: ${arg}`);
  }
  return opts;
}

export function main(argv = process.argv.slice(2)) {
  let opts;
  try {
    opts = parseArgs(argv);
  } catch (err) {
    console.error(`check-doc-links: ${err.message}\n`);
    console.error(HELP);
    return 2;
  }
  if (opts.help) {
    console.log(HELP);
    return 0;
  }

  const { files, links, broken, site } = findBrokenLinks(opts);

  console.log(
    `mode: ${opts.includeCodeFences ? 'include-code-fences (naive)' : 'markdown-correct (skip fences + inline code)'}`,
  );
  console.log(`root: ${opts.root}`);
  console.log(`scanned ${files.length} markdown files, checked ${links.length} relative links`);
  if (site) {
    const scope = (name) => links.filter((link) => link.scope === name).length;
    console.log(
      `site map: ${site.routes.size} published pages; ` +
        `links → site ${scope('site')}, route ${scope('route')}, github ${scope('github')}, ` +
        `local ${scope('local')}`,
    );
  } else {
    console.log('site map: not applicable (scanned root does not contain docs/)');
  }
  console.log(`broken: ${broken.length}`);

  if (opts.verbose) {
    for (const link of links) {
      console.log(`  ${link.kind ? 'BROKEN' : 'ok    '}  ${link.file}:${link.line}  ${link.target}`);
    }
  }

  const byFile = new Map();
  for (const record of broken) {
    if (!byFile.has(record.file)) byFile.set(record.file, []);
    byFile.get(record.file).push(record);
  }
  for (const [file, records] of [...byFile.entries()].sort()) {
    console.log(`\n${file}`);
    for (const record of records) {
      console.log(`  L${record.line}  [${record.kind}] ${record.target}  ->  ${record.resolved}`);
    }
  }

  return broken.length ? 1 : 0;
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  process.exitCode = main();
}
