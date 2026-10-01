import { afterEach, describe, expect, it } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { findBrokenLinks, maskCode, slugify } from './check-doc-links.mjs';

const tmpDirs = [];

function makeRepo(files) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'check-doc-links-'));
  tmpDirs.push(dir);
  for (const [name, body] of Object.entries(files)) {
    fs.writeFileSync(path.join(dir, name), body);
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
