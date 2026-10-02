/**
 * Markdown-it plugins that adapt the existing `docs/` tree to VitePress
 * **without editing a single document**.
 *
 * 1. `docLinks` — resolve every relative link against the *real* source file
 *    (`env.realPath`), not the rewritten route. The `zh-CN/` → `/` rewrite
 *    changes the directory depth, so VitePress' own resolution mis-resolves
 *    links that cross between the two language trees (e.g. `zh-CN/ui/README.md`
 *    → `../../en/ui/README.md`). Links that leave the published set are
 *    rewritten to GitHub blob URLs instead of becoming dead links.
 *
 * 2. `strayHtml` — the docs are plain Markdown, but a handful of prose
 *    placeholders look like HTML tags (`<plugin name>`, `<String>`, `<Value>`).
 *    markdown-it passes raw HTML through, and Vue's SFC compiler then fails with
 *    "Element is missing end tag". Tags outside the HTML element set are turned
 *    back into literal text.
 */
import path from 'node:path';
import {
  REPO_ROOT,
  SRC_DIR,
  buildSiteIndex,
  githubBlobUrl,
  isExternalHref,
  isOnSite,
  linkPath,
  linkSuffix,
  pageRoute,
} from '../site-map.mjs';

interface Token {
  type: string;
  content: string;
  children?: Token[] | null;
}

interface Env {
  realPath?: string;
}

/** Elements that may stay raw HTML in a Vue SFC template. */
const HTML_TAGS = new Set([
  'a', 'abbr', 'address', 'area', 'article', 'aside', 'audio', 'b', 'base', 'bdi', 'bdo', 'blockquote',
  'body', 'br', 'button', 'canvas', 'caption', 'cite', 'code', 'col', 'colgroup', 'data', 'datalist',
  'dd', 'del', 'details', 'dfn', 'dialog', 'div', 'dl', 'dt', 'em', 'embed', 'fieldset', 'figcaption',
  'figure', 'footer', 'form', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'head', 'header', 'hgroup', 'hr',
  'html', 'i', 'iframe', 'img', 'input', 'ins', 'kbd', 'label', 'legend', 'li', 'link', 'main', 'map',
  'mark', 'menu', 'meta', 'meter', 'nav', 'noscript', 'object', 'ol', 'optgroup', 'option', 'output',
  'p', 'picture', 'pre', 'progress', 'q', 'rp', 'rt', 'ruby', 's', 'samp', 'script', 'search', 'section',
  'select', 'slot', 'small', 'source', 'span', 'strong', 'style', 'sub', 'summary', 'sup', 'table',
  'tbody', 'td', 'template', 'textarea', 'tfoot', 'th', 'thead', 'time', 'title', 'tr', 'track', 'u',
  'ul', 'var', 'video', 'wbr',
]);

const TAG_NAME_RE = /^<\/?([A-Za-z][A-Za-z0-9-]*)/;

/**
 * Resolve one href written in `realFile` to its final form:
 * an absolute site route, a GitHub blob URL, or the original href when it is
 * external / anchor-only / outside the repository.
 */
export function resolveDocHref(href: string, realFile: string, index: ReturnType<typeof buildSiteIndex>): string {
  if (!href || href.startsWith('#') || isExternalHref(href)) return href;
  const target = linkPath(href);
  const suffix = linkSuffix(href);
  if (!target) return href;
  // Already a site route (`/user/skills`): VitePress owns it from here.
  if (target.startsWith('/')) return href;

  let decoded = target;
  try {
    decoded = decodeURIComponent(target);
  } catch {
    /* keep the raw target when it is not valid percent-encoding */
  }

  const abs = path.resolve(path.dirname(realFile), decoded);
  const rel = path.relative(SRC_DIR, abs).split(path.sep).join('/');
  if (!rel.startsWith('..') && isOnSite(rel)) return `${pageRoute(rel)}${suffix}`;

  const repoRel = path.relative(REPO_ROOT, abs).split(path.sep).join('/');
  if (repoRel.startsWith('..')) return href; // outside the repository: leave it visible
  return `${githubBlobUrl(repoRel)}${suffix}`;
}

/** Rewrite `link_open` / `image` targets before VitePress normalises them. */
export function docLinks(md: any): void {
  const index = buildSiteIndex();
  const rewrite = (tokens: Token[], idx: number, env: Env) => {
    const token = tokens[idx];
    const attrs = (token as unknown as { attrs?: [string, string][] }).attrs;
    if (!attrs) return;
    const realFile = env?.realPath;
    if (!realFile) return;
    for (const attr of attrs) {
      if (attr[0] !== 'href' && attr[0] !== 'src') continue;
      attr[1] = resolveDocHref(attr[1], realFile, index);
    }
  };

  const wrap = (name: 'link_open' | 'image') => {
    const original =
      md.renderer.rules[name] ??
      ((tokens: Token[], idx: number, options: unknown, _env: Env, self: any) =>
        self.renderToken(tokens, idx, options));
    md.renderer.rules[name] = (tokens: Token[], idx: number, options: unknown, env: Env, self: any) => {
      rewrite(tokens, idx, env);
      return original(tokens, idx, options, env, self);
    };
  };
  wrap('link_open');
  wrap('image');
}

/** Turn prose placeholders that look like unknown HTML tags back into text. */
export function strayHtml(md: any): void {
  md.core.ruler.push('pointer-stray-html', (state: { tokens: Token[] }) => {
    const isStray = (token: Token) => {
      if (token.type !== 'html_inline' && token.type !== 'html_block') return false;
      const match = TAG_NAME_RE.exec(token.content);
      return match ? !HTML_TAGS.has(match[1].toLowerCase()) : false;
    };
    for (const token of state.tokens) {
      if (isStray(token)) token.type = 'text';
      if (token.type === 'inline' && Array.isArray(token.children)) {
        for (const child of token.children) {
          if (isStray(child)) child.type = 'text';
        }
      }
    }
  });
}
