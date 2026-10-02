/**
 * Auto-generated sidebars.
 *
 * Entries are derived from the published page set (`site-map.mjs`) instead of
 * being hand-listed, so a new document shows up in the sidebar without a
 * config edit — and a page that is *not* published can never leak in as a dead
 * link. Titles come from each file's first H1, falling back to the file name.
 */
import fs from 'node:fs';
import path from 'node:path';
import { SRC_DIR, isOnSite, pageRoute } from '../site-map.mjs';

export interface SidebarItem {
  text: string;
  link?: string;
  collapsed?: boolean;
  items?: SidebarItem[];
}

export interface SectionSpec {
  text: string;
  /**
   * Explicit pages, in the order given (`docs/`-relative markdown paths).
   * Use when a section is a hand-picked subset of a directory — a directory
   * listing cannot express "these two pages belong to Getting Started, the
   * rest to Guides".
   */
  files?: string[];
  /**
   * Groups inside a section. A group is either a whole directory listing
   * (`dir`, minus `exclude`d pages), an explicit page list (`files`), or both.
   *
   * - `exclude` / `files` hold `docs/`-relative paths, so a page can be pulled
   *   out of its own directory and listed elsewhere — placement without moving
   *   files (URLs stay stable).
   * - With `label`, the group renders as a collapsible second-level heading.
   * - `dir` is optional: a `files`-only entry is a hand-picked sub-group.
   */
  dirs?: { dir?: string; label?: string; exclude?: string[]; files?: string[] }[];
  /** Hand-written entries rendered after `files` and before the directories. */
  items?: SidebarItem[];
}

/** Preferred order inside a directory; anything not listed follows alphabetically. */
const PRIORITY: Record<string, string[]> = {
  'zh-CN/user': [
    'README.md', 'getting-started.md', 'which-build.md', 'model-providers.md',
    'settings.md', 'rules.md', 'workspace.md', 'scheduled-tasks.md', 'cloud-host.md',
    'skills.md', 'im-channels.md', 'subagents.md', 'mcp.md', 'plugins.md',
    'webhook.md', 'standalone-server.md', 'project-lint.md',
  ],
  // The 开发 section's four groups are hand-picked in config.mts (`ZH_DEV_*`),
  // so this list now only orders the 其他 fallback group of `zh-CN/developer`.
  'zh-CN/developer': ['cli.md', 'standalone-local-login.md'],
  'zh-CN/contributing': [
    'README.md', 'cross-platform-build.md', 'versioning.md', 'ci.md',
    'docs-site.md', 'coder-agent-offline-eval-setup.md',
  ],
  'zh-CN/internals': ['README.md', 'llm-prompt-assembly-order.md', 'agent-task-board-and-verification.md'],
  'zh-CN/ui': ['README.md'],
  'en/user': [
    'README.md', 'getting-started.md', 'which-build.md', 'model-providers.md',
    'settings.md', 'rules.md', 'workspace.md', 'scheduled-tasks.md', 'cloud-host.md',
    'skills.md', 'im-channels.md', 'subagents.md', 'mcp.md', 'plugins.md',
    'webhook.md', 'standalone-server.md', 'project-lint.md',
  ],
  // The Development section's groups are hand-picked in config.mts (`EN_DEV_*`),
  // so this list only orders the Other fallback group of `en/developer`.
  'en/developer': ['cli.md', 'standalone-local-login.md'],
  'en/contributing': [
    'README.md', 'cross-platform-build.md', 'versioning.md', 'ci.md', 'docs-site.md',
    'coder-agent-offline-eval-setup.md',
  ],
};

/** First `# heading` outside fenced code, with inline markdown stripped. */
function titleOf(fileAbs: string): string | null {
  let text: string;
  try {
    text = fs.readFileSync(fileAbs, 'utf8');
  } catch {
    return null;
  }
  let fence: string | null = null;
  for (const line of text.split('\n')) {
    const fenceMatch = /^\s{0,3}(`{3,}|~{3,})/.exec(line);
    if (fence) {
      if (fenceMatch && fenceMatch[1][0] === fence[0] && fenceMatch[1].length >= fence.length) fence = null;
      continue;
    }
    if (fenceMatch) {
      fence = fenceMatch[1];
      continue;
    }
    const heading = /^#\s+(.+?)\s*#*\s*$/.exec(line);
    if (!heading) continue;
    return heading[1]
      .replace(/`([^`]*)`/g, '$1')
      .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
      .replace(/[*_~]/g, '')
      .trim();
  }
  return null;
}

function itemsInDir(dirRel: string, exclude: string[] = []): SidebarItem[] {
  const dirAbs = path.join(SRC_DIR, dirRel);
  let entries: string[];
  try {
    entries = fs.readdirSync(dirAbs);
  } catch {
    return [];
  }
  // `exclude` holds `docs/`-relative paths, so the same list can name pages
  // that live in another directory without ambiguity.
  const skip = new Set(exclude);
  const files = entries.filter(
    (name) => name.endsWith('.md') && !skip.has(`${dirRel}/${name}`) && isOnSite(`${dirRel}/${name}`),
  );
  const priority = PRIORITY[dirRel] ?? [];
  files.sort((a, b) => {
    const ia = priority.indexOf(a);
    const ib = priority.indexOf(b);
    if (ia !== -1 || ib !== -1) return (ia === -1 ? Infinity : ia) - (ib === -1 ? Infinity : ib);
    return a.localeCompare(b);
  });
  return files.map((name) => entryFor(`${dirRel}/${name}`)).filter((item) => item !== null);
}

/** Sidebar entry for one `docs/`-relative markdown path, or `null` when it is off the site. */
function entryFor(rel: string): SidebarItem | null {
  if (!isOnSite(rel)) return null;
  return {
    text: titleOf(path.join(SRC_DIR, rel)) ?? path.basename(rel).replace(/\.md$/, ''),
    link: pageRoute(rel),
  };
}

/** Entries for an explicit page list, in the caller's order; off-site pages are dropped. */
function itemsInFiles(files: string[]): SidebarItem[] {
  return files.map((rel) => entryFor(rel)).filter((item): item is SidebarItem => item !== null);
}

/** Sections → VitePress sidebar. A section with no published page is omitted. */
export function buildSidebar(sections: SectionSpec[]): SidebarItem[] {
  const out: SidebarItem[] = [];
  for (const section of sections) {
    const body: SidebarItem[] = [
      ...itemsInFiles(section.files ?? []),
      ...(section.items ?? []),
    ];
    for (const { dir, label, exclude, files } of section.dirs ?? []) {
      const generated = [...(dir ? itemsInDir(dir, exclude) : []), ...itemsInFiles(files ?? [])];
      if (!generated.length) continue;
      if (label) body.push({ text: label, collapsed: false, items: generated });
      else body.push(...generated);
    }
    if (body.length) out.push({ text: section.text, collapsed: false, items: body });
  }
  return out;
}
