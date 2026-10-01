#!/usr/bin/env node
/**
 * Guardrail: every component a `.vue` template renders must resolve at runtime.
 *
 * A PascalCase tag that the SFC never imports (and that no plugin registers
 * globally) still compiles and builds cleanly — `vue-tsc` does not flag it —
 * but at runtime Vue falls back to `resolveComponent(name)`, gets the string
 * back, and renders an inert `<askuserbanner>` element. The surface is silently
 * missing with no error anywhere (`ChatView.vue` shipped `<AskUserBanner />`
 * that way: the top-of-chat ask_user banner never mounted).
 *
 * This checker compiles each template with the real SFC compiler and reports
 * every `_resolveComponent("<PascalCase>")` that is neither an imported binding
 * nor a self-reference (an SFC may render itself by file name — Vue resolves
 * that at runtime through the component's own name).
 *
 * Usage: node scripts/check-vue-template-imports.mjs [rootDir]
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { compileScript, compileTemplate, parse } from 'vue/compiler-sfc';

/** Built-ins the compiler resolves on its own (no import in the SFC). */
const BUILT_IN_COMPONENTS = new Set(['Transition', 'TransitionGroup', 'KeepAlive', 'Teleport', 'Suspense']);

const RESOLVE_COMPONENT_RE = /_resolveComponent\("([^"]+)"\)/g;

/**
 * Component tags in one SFC template that would not resolve at runtime.
 *
 * @param {string} source raw `.vue` file contents
 * @param {string} filename used for the compiler id and the self-reference check
 * @returns {string[]} unresolved component names, deduped, in template order
 */
export function findUnresolvedTemplateComponents(source, filename = 'Component.vue') {
  const { descriptor, errors } = parse(source, { filename });
  if (errors.length > 0 || !descriptor.template) return [];

  let bindingMetadata;
  if (descriptor.script || descriptor.scriptSetup) {
    bindingMetadata = compileScript(descriptor, {
      id: filename,
      inlineTemplate: false
    }).bindings;
  }

  const { code } = compileTemplate({
    source: descriptor.template.content,
    filename,
    id: filename,
    compilerOptions: bindingMetadata ? { bindingMetadata } : {}
  });

  const selfName = path.basename(filename, '.vue');
  const unresolved = new Set();
  for (const match of code.matchAll(RESOLVE_COMPONENT_RE)) {
    const name = match[1];
    if (name === selfName) continue;
    if (BUILT_IN_COMPONENTS.has(name)) continue;
    if (!/^[A-Z]/.test(name)) continue;
    unresolved.add(name);
  }
  return [...unresolved];
}

/** Every `.vue` file under `root`, recursively. */
export function listVueFiles(root) {
  return fs
    .readdirSync(root, { recursive: true, withFileTypes: true })
    .filter(entry => entry.isFile() && entry.name.endsWith('.vue'))
    .map(entry => path.join(entry.parentPath ?? entry.path, entry.name))
    .sort();
}

/** Unresolved components for every SFC under `root`, as `{ file, components }`. */
export function scanVueTemplateImports(root) {
  const violations = [];
  for (const file of listVueFiles(root)) {
    const unresolved = findUnresolvedTemplateComponents(fs.readFileSync(file, 'utf8'), file);
    if (unresolved.length > 0) violations.push({ file, components: unresolved });
  }
  return violations;
}

function main() {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const root = process.argv[2] ? path.resolve(process.argv[2]) : path.join(here, '..', 'src');
  const violations = scanVueTemplateImports(root);
  if (violations.length === 0) {
    console.log(`[check-vue-template-imports] ok: ${listVueFiles(root).length} SFC(s) under ${root}`);
    return;
  }
  for (const { file, components } of violations) {
    console.error(`[check-vue-template-imports] ${file}: template renders <${components.join('>, <')}> without importing it`);
  }
  process.exitCode = 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
