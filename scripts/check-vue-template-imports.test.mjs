import { describe, expect, it } from 'vitest';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  findUnresolvedTemplateComponents,
  scanVueTemplateImports
} from './check-vue-template-imports.mjs';

const REPO_ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

function sfc(script, template) {
  return `<script setup lang="ts">\n${script}\n</script>\n\n<template>\n${template}\n</template>\n`;
}

describe('findUnresolvedTemplateComponents', () => {
  it('flags a component the template renders without importing it', () => {
    // Regression shape: ChatView.vue rendered <AskUserBanner /> with no import, so Vue
    // resolved it at runtime and mounted an inert <askuserbanner> element instead.
    const source = sfc(
      `import Composer from './Composer.vue'`,
      `<div>\n  <Composer />\n  <AskUserBanner />\n</div>`
    );
    expect(findUnresolvedTemplateComponents(source, 'ChatView.vue')).toEqual(['AskUserBanner']);
  });

  it('accepts imported components, local declarations and kebab-case tags', () => {
    const source = sfc(
      [
        `import AskUserBanner from './AskUserBanner.vue'`,
        `import { defineComponent } from 'vue'`,
        `const MessageListSkeleton = defineComponent({ name: 'MessageListSkeleton' })`
      ].join('\n'),
      `<div>\n  <AskUserBanner />\n  <MessageListSkeleton />\n  <some-native-tag />\n</div>`
    );
    expect(findUnresolvedTemplateComponents(source, 'ChatView.vue')).toEqual([]);
  });

  it('accepts an SFC that renders itself and vue built-ins', () => {
    const recursive = sfc(``, `<div>\n  <WorkspaceTreeNode />\n  <Transition name="fade">\n    <span />\n  </Transition>\n</div>`);
    expect(findUnresolvedTemplateComponents(recursive, 'WorkspaceTreeNode.vue')).toEqual([]);
  });

  it('reports each unresolved component once, in template order', () => {
    const source = sfc(``, `<div>\n  <Beta />\n  <Alpha />\n  <Beta />\n</div>`);
    expect(findUnresolvedTemplateComponents(source, 'Widget.vue')).toEqual(['Beta', 'Alpha']);
  });
});

describe('scanVueTemplateImports', () => {
  it('finds no unresolved template component in src/', () => {
    expect(scanVueTemplateImports(path.join(REPO_ROOT, 'src'))).toEqual([]);
  });
});
