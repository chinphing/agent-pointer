#!/usr/bin/env node
/**
 * Copy generated Tauri icons into public/ for Vite and web builds.
 */
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const iconsDir = join(root, 'src-tauri', 'icons');
const publicDir = join(root, 'public');

const copies = [
  ['128x128.png', 'app-icon.png'],
  ['128x128@2x.png', 'app-icon@2x.png'],
  ['icon.ico', 'favicon.ico'],
];

mkdirSync(publicDir, { recursive: true });

for (const [from, to] of copies) {
  copyFileSync(join(iconsDir, from), join(publicDir, to));
}

console.log('Synced web icons to public/');
