#!/usr/bin/env node
/**
 * Generate app icons from a source PNG:
 * - Rounded corners (transparent outside the radius)
 * - macOS: 10% transparent padding on each side (icon.icns)
 * - Windows/Linux: full-bleed, no extra padding
 */
import { copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execSync } from 'node:child_process';
import sharp from 'sharp';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const iconsDir = join(root, 'src-tauri', 'icons');

const SOURCE_REF_WIDTH = 1254;
const CORNER_RADIUS_REF = 250;
const MACOS_PADDING_RATIO = 0.1;
const OUTPUT_SIZE = 1024;

const defaultSource = resolve('/Users/starliu/Desktop/logo3.png');
const sourcePath = resolve(process.argv[2] ?? defaultSource);

function roundedMask(size, radius) {
  return Buffer.from(
    `<svg width="${size}" height="${size}"><rect x="0" y="0" width="${size}" height="${size}" rx="${radius}" ry="${radius}" fill="white"/></svg>`
  );
}

async function applyRoundedCorners(input, size, radius) {
  const resized = await sharp(input)
    .resize(size, size, { fit: 'cover' })
    .png()
    .toBuffer();

  return sharp(resized)
    .composite([{ input: roundedMask(size, radius), blend: 'dest-in' }])
    .png()
    .toBuffer();
}

async function withMacPadding(roundedPng, size, paddingRatio) {
  const inner = Math.round(size * (1 - 2 * paddingRatio));
  const offset = Math.round(size * paddingRatio);

  const scaled = await sharp(roundedPng)
    .resize(inner, inner, { fit: 'fill' })
    .png()
    .toBuffer();

  return sharp({
    create: {
      width: size,
      height: size,
      channels: 4,
      background: { r: 0, g: 0, b: 0, alpha: 0 },
    },
  })
    .composite([{ input: scaled, left: offset, top: offset }])
    .png()
    .toBuffer();
}

async function main() {
  const radius = Math.round((CORNER_RADIUS_REF * OUTPUT_SIZE) / SOURCE_REF_WIDTH);

  console.log(`Source: ${sourcePath}`);
  console.log(`Output size: ${OUTPUT_SIZE}, corner radius: ${radius}px`);

  const rounded = await applyRoundedCorners(sourcePath, OUTPUT_SIZE, radius);

  mkdirSync(iconsDir, { recursive: true });

  const iconPath = join(iconsDir, 'icon.png');
  await sharp(rounded).toFile(iconPath);
  console.log(`Written ${iconPath} (Windows/Linux)`);

  const macRounded = await withMacPadding(rounded, OUTPUT_SIZE, MACOS_PADDING_RATIO);
  const macPath = join(iconsDir, 'icon-macos.png');
  await sharp(macRounded).toFile(macPath);
  console.log(`Written ${macPath} (macOS, 10% padding)`);

  console.log('Running tauri icon for Windows/Linux assets...');
  execSync(`npx tauri icon "${iconPath}" -o "${iconsDir}"`, {
    cwd: join(root, 'src-tauri'),
    stdio: 'inherit',
  });

  const macIconsDir = join(iconsDir, '_macos_tmp');
  mkdirSync(macIconsDir, { recursive: true });
  console.log('Running tauri icon for macOS icns...');
  execSync(`npx tauri icon "${macPath}" -o "${macIconsDir}"`, {
    cwd: join(root, 'src-tauri'),
    stdio: 'inherit',
  });

  copyFileSync(join(macIconsDir, 'icon.icns'), join(iconsDir, 'icon.icns'));
  console.log('Copied icon.icns (macOS with padding)');
  rmSync(macIconsDir, { recursive: true, force: true });

  execSync('node scripts/sync-web-icons.mjs', { cwd: root, stdio: 'inherit' });
  console.log('Done.');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
