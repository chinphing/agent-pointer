import { afterEach, describe, expect, it } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { findBrokenLinks, maskCode, slugify } from './check-doc-links.mjs';
import { alternateLocale, buildSiteIndex } from '../docs-site/site-map.mjs';

const tmpDirs = [];

function makeRepo(files) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'check-doc-links-'));
  tmpDirs.push(dir);
  for (const [name, body] of Object.entries(files)) {
    const file = path.join(dir, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, body);
  }
  return dir;
}

afterEach(() => {
  while (tmpDirs.length) fs.rmSync(tmpDirs.pop(), { recursive: true, force: true });
});

describe('slugify', () => {
  it('lowercases and turns every space into a dash without collapsing', () => {
    expect(slugify('History Trim')).toBe('history-trim');
    expect(slugify('a  b')).toBe('a--b');
  });

  it('keeps ASCII - and _ but strips other punctuation', () => {
    expect(slugify('task_board 触发的历史截断（当前实现）')).toBe('task_board-触发的历史截断当前实现');
    expect(slugify('ALB / 就绪（平台）')).toBe('alb--就绪平台');
  });

  it('drops inline markdown and inline HTML', () => {
    expect(slugify('**Bold** `code` <em>x</em>')).toBe('bold-code-x');
  });
});

describe('maskCode', () => {
  it('blanks fenced blocks and inline code', () => {
    const masked = maskCode(['before `[a](./x.md)`', '```md', '[b](./y.md)', '```', 'after'].join('\n'));
    expect(masked).not.toContain('./x.md');
    expect(masked).not.toContain('./y.md');
    expect(masked.split('\n')[4]).toBe('after');
  });
});

describe('findBrokenLinks', () => {
  it('skips code, ignores absolute URLs, flags missing files and anchors', () => {
    const root = makeRepo({
      'target.md': '# Hello World\n\n## 小节（一）\n',
      'index.md': [
        '[ok](./target.md#hello-world)',
        '[anchor ok](./target.md#小节一)',
        '[missing file](./nope.md)',
        '[missing anchor](./target.md#nope)',
        '[external](https://example.com/x.md)',
        '[mail](mailto:a@b.c)',
        '',
        '```md',
        '[fenced](./nope.md)',
        '```',
        '',
        'inline `[inline](./nope.md)` stays literal',
        '',
      ].join('\n'),
    });

    const { files, links, broken } = findBrokenLinks({ root });
    expect(files).toHaveLength(2);
    expect(links).toHaveLength(4);
    expect(broken.map((b) => [b.file, b.line, b.kind])).toEqual([
      ['index.md', 3, 'missing-path'],
      ['index.md', 4, 'missing-anchor'],
    ]);
  });

  it('reports the same fence contents as broken in naive mode', () => {
    const root = makeRepo({
      'index.md': '```md\n[upstream](LICENSE-APACHE)\n```\n',
    });

    expect(findBrokenLinks({ root }).broken).toHaveLength(0);
    expect(findBrokenLinks({ root, includeCodeFences: true }).broken).toHaveLength(1);
  });
});

/** Minimal site context: routes → docs-relative files, rooted at a temp repo. */
function siteContext(root, entries) {
  return { srcDir: root, repoRoot: root, routes: new Map(entries) };
}

describe('site routes', () => {
  it('flags a /route link that no published page serves', () => {
    const root = makeRepo({
      'index.md': '[ok](/user/skills)\n[dead](/user/nope)\n',
      'zh-CN/user/skills.md': '# Skills\n',
    });
    const site = siteContext(root, [['/user/skills', 'zh-CN/user/skills.md']]);

    const { links, broken } = findBrokenLinks({ root, site });
    expect(links.map((link) => link.scope)).toEqual(['route', 'route']);
    expect(broken.map((record) => [record.line, record.kind, record.resolved])).toEqual([
      [2, 'missing-route', '/user/nope'],
    ]);
  });

  it('resolves anchors through a route, including the trailing-slash form', () => {
    const root = makeRepo({
      'index.md': '[ok](/user/skills#安装)\n[dead](/user/skills#nope)\n[index](/user/#skills)\n',
      'zh-CN/user/skills.md': '# Skills\n\n## 安装\n',
      'zh-CN/user/README.md': '# User\n\n## Skills\n',
    });
    const site = siteContext(root, [
      ['/user/skills', 'zh-CN/user/skills.md'],
      ['/user', 'zh-CN/user/README.md'],
    ]);

    const { broken } = findBrokenLinks({ root, site });
    expect(broken.map((record) => [record.line, record.kind])).toEqual([[2, 'missing-anchor']]);
  });

  it('classifies relative links as on-site or GitHub-bound', () => {
    const root = makeRepo({
      'zh-CN/user/skills.md': '[dev](../developer/architecture.md)\n[design](../design/x.md)\n',
      'zh-CN/developer/architecture.md': '# Architecture\n',
      'zh-CN/design/x.md': '# Design\n',
    });
    const site = siteContext(root, []);

    const { links, broken } = findBrokenLinks({ root, site });
    expect(links.map((link) => [link.target, link.scope])).toEqual([
      ['../developer/architecture.md', 'site'],
      ['../design/x.md', 'github'], // tier-3 dir: rewritten to GitHub at build time
    ]);
    expect(broken).toHaveLength(0);
  });

  it('still reports a missing file for a GitHub-bound link', () => {
    const root = makeRepo({
      'zh-CN/user/skills.md': '[gone](../design/gone.md)\n',
    });

    const { broken } = findBrokenLinks({ root, site: siteContext(root, []) });
    expect(broken.map((record) => [record.line, record.kind])).toEqual([[1, 'missing-path']]);
  });
});

describe('site map', () => {
  it('mounts the docs tree on unique routes and keeps tier-3 off the site', () => {
    const { byRoute, rels } = buildSiteIndex();

    // English is the default locale: `en/` is stripped, `zh-CN/` is kept.
    expect(byRoute.get('/')).toBe('en/README.md');
    expect(byRoute.get('/zh-CN/')).toBe('zh-CN/README.md');
    expect(byRoute.get('/user/')).toBe('en/user/README.md');
    expect(byRoute.get('/zh-CN/user/')).toBe('zh-CN/user/README.md');
    expect(byRoute.get('/user/skills')).toBe('en/user/skills.md');
    expect(byRoute.get('/zh-CN/user/skills')).toBe('zh-CN/user/skills.md');
    expect(byRoute.has('/en/')).toBe(false); // the `/en/` locale prefix is gone

    const files = [...rels];
    expect(files).toHaveLength(byRoute.size); // no route collisions
    expect(files.some((file) => file === 'README.md')).toBe(false); // docs/ language picker
    for (const tier3 of ['zh-CN/design/', 'zh-CN/plans/', 'zh-CN/settings-refactor/', 'zh-CN/superpowers/', 'en/design/']) {
      expect(files.some((file) => file.startsWith(tier3))).toBe(false);
    }
  });
});

describe('alternateLocale', () => {
  const index = buildSiteIndex();

  it('points a translated page at its counterpart in the other locale', () => {
    expect(alternateLocale('user/skills.md', index)).toEqual({ text: '简体中文', link: '/zh-CN/user/skills' });
    expect(alternateLocale('zh-CN/user/skills.md', index)).toEqual({ text: 'English', link: '/user/skills' });
    // Directory indexes round-trip through README.md.
    expect(alternateLocale('user/index.md', index)).toEqual({ text: '简体中文', link: '/zh-CN/user/' });
    expect(alternateLocale('zh-CN/index.md', index)).toEqual({ text: 'English', link: '/' });
  });

  it('falls back to the other locale home when the page is not translated', () => {
    // English-only page: zh-CN/DEVELOPMENT.md does not exist.
    expect(alternateLocale('DEVELOPMENT.md', index)).toEqual({ text: '简体中文', link: '/zh-CN/' });
    // The five user-facing trees are fully translated; the maintainer-notes
    // trees (internals / agents / llm / ui) are still Chinese-only.
    expect(alternateLocale('zh-CN/internals/long-chat-memory.md', index)).toEqual({ text: 'English', link: '/' });
  });

  it('returns null for pages outside the published locale trees', () => {
    expect(alternateLocale('404.md', index)).toBeNull();
  });
});
