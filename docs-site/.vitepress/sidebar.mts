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
  /** Directories (relative to `docs/`) whose pages become the section body. */
  dirs?: { dir: string; label?: string }[];
  /** Hand-written entries rendered before the generated ones. */
  items?: SidebarItem[];
}

/** Preferred order inside a directory; anything not listed follows alphabetically. */
const PRIORITY: Record<string, string[]> = {
  'zh-CN/user': [
    'README.md', 'getting-started.md', 'which-build.md', 'cloud-host.md',
    'skills.md', 'im-channels.md', 'subagents.md', 'mcp.md', 'plugins.md',
    'webhook.md', 'standalone-server.md', 'project-lint.md',
  ],
  'zh-CN/developer': [
    'README.md', 'architecture.md', 'native-tool-calling-protocol.md',
    'agent-extension-hooks.md', 'mcp.md', 'standalone-deployment.md',
    'standalone-local-login.md', 'workspace-root.md', 'logging.md',
  ],
  'zh-CN/contributing': [
    'README.md', 'cross-platform-build.md', 'versioning.md',
    'coder-agent-offline-eval-setup.md', 'web-media-and-desktop-snapshot.md',
    'macos-window-chrome.md',
  ],
  'zh-CN/internals': ['README.md', 'llm-prompt-assembly-order.md', 'agent-task-board-and-verification.md'],
  'zh-CN/ui': ['README.md'],
  'en/user': [
    'README.md', 'getting-started.md', 'which-build.md', 'cloud-host.md',
    'skills.md', 'im-channels.md', 'subagents.md', 'mcp.md', 'plugins.md',
    'standalone-server.md',
  ],
  'en/developer': ['README.md', 'architecture.md', 'standalone-deployment.md'],
  'en/contributing': ['README.md', 'cross-platform-build.md', 'versioning.md'],
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

function itemsInDir(dirRel: string): SidebarItem[] {
  const dirAbs = path.join(SRC_DIR, dirRel);
  let entries: string[];
  try {
    entries = fs.readdirSync(dirAbs);
  } catch {
    return [];
  }
  const files = entries.filter((name) => name.endsWith('.md') && isOnSite(`${dirRel}/${name}`));
  const priority = PRIORITY[dirRel] ?? [];
  files.sort((a, b) => {
    const ia = priority.indexOf(a);
    const ib = priority.indexOf(b);
    if (ia !== -1 || ib !== -1) return (ia === -1 ? Infinity : ia) - (ib === -1 ? Infinity : ib);
    return a.localeCompare(b);
  });
  return files.map((name) => {
    const rel = `${dirRel}/${name}`;
    return {
      text: titleOf(path.join(dirAbs, name)) ?? name.replace(/\.md$/, ''),
      link: pageRoute(rel),
    };
  });
}

/** Sections → VitePress sidebar. `skipRels` are `docs/`-relative paths already placed by hand. */
export function buildSidebar(sections: SectionSpec[], skipRels: string[] = []): SidebarItem[] {
  const skip = new Set(skipRels);
  const out: SidebarItem[] = [];
  for (const section of sections) {
    const body: SidebarItem[] = [...(section.items ?? [])];
    for (const { dir, label } of section.dirs ?? []) {
      const generated = itemsInDir(dir).filter((item) => {
        const rel = [...skip].find((s) => pageRoute(s) === item.link);
        if (rel) {
          skip.delete(rel);
          return false;
        }
        return true;
      });
      if (!generated.length) continue;
      if (label) body.push({ text: label, collapsed: false, items: generated });
      else body.push(...generated);
    }
    if (body.length) out.push({ text: section.text, collapsed: false, items: body });
  }
  return out;
}
