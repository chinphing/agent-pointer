import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  markBuiltReleaseFiles,
  releaseAssetName,
  resolveReleaseLabel,
  serverDebBaseName,
  serverZipBaseName,
  stageReleaseAssets,
} from './rename-release-assets.mjs'

describe('releaseAssetName', () => {
  it('marks client installers and updater signatures', () => {
    expect(releaseAssetName('Pointer_0.1.2_amd64.deb', 'community')).toBe(
      'Pointer-community-0.1.2-amd64.deb',
    )
    expect(releaseAssetName('Pointer_0.1.2_x64-setup.exe', 'official')).toBe(
      'Pointer-official-0.1.2-x64-setup.exe',
    )
    expect(releaseAssetName('Pointer_0.1.2_x64_zh-CN.msi', 'community')).toBe(
      'Pointer-community-0.1.2-x64-zh-CN.msi',
    )
    expect(releaseAssetName('Pointer_0.1.2_amd64.AppImage', 'official')).toBe(
      'Pointer-official-0.1.2-amd64.AppImage',
    )
    expect(releaseAssetName('Pointer_0.1.2_amd64.deb.sig', 'official')).toBe(
      'Pointer-official-0.1.2-amd64.deb.sig',
    )
  })

  it('marks server zip and deb names', () => {
    expect(releaseAssetName('pointer-server-linux-x64.zip', 'community')).toBe(
      'pointer-server-community-linux-x64.zip',
    )
    expect(releaseAssetName('pointer-server-windows-x64.zip', 'official')).toBe(
      'pointer-server-official-windows-x64.zip',
    )
    expect(releaseAssetName('pointer-server_0.1.2_amd64.deb', 'official')).toBe(
      'pointer-server-official-0.1.2-amd64.deb',
    )
  })

  it('leaves an already marked name unchanged', () => {
    expect(releaseAssetName('Pointer-community-0.1.2-amd64.deb', 'community')).toBe(
      'Pointer-community-0.1.2-amd64.deb',
    )
  })

  it('marks the updater tarball and its signature, which carry no separator', () => {
    expect(releaseAssetName('Pointer.app.tar.gz', 'community')).toBe(
      'Pointer-community.app.tar.gz',
    )
    expect(releaseAssetName('Pointer.app.tar.gz.sig', 'official')).toBe(
      'Pointer-official.app.tar.gz.sig',
    )
    // Idempotent: a second pass must not stack another flavor marker.
    expect(releaseAssetName('Pointer-community.app.tar.gz', 'community')).toBe(
      'Pointer-community.app.tar.gz',
    )
    expect(releaseAssetName('Pointer-official.app.tar.gz.sig', 'official')).toBe(
      'Pointer-official.app.tar.gz.sig',
    )
  })
})

describe('resolveReleaseLabel', () => {
  it('marks an unbound build as community and the readflowai build as official', () => {
    expect(resolveReleaseLabel({})).toBe('community')
    expect(resolveReleaseLabel({ POINTER_PACKAGE_LABEL: 'official' })).toBe('official')
    expect(resolveReleaseLabel({
      POINTER_EDITION: 'managed',
      POINTER_API_BASE: 'https://pointer-api.readflowai.com',
      POINTER_WEB_BASE: 'https://pointer.readflowai.com/',
    })).toBe('official')
    expect(resolveReleaseLabel({
      POINTER_EDITION: 'managed',
      POINTER_API_BASE: 'https://pointer-api.example.com',
      POINTER_WEB_BASE: 'https://pointer.example.com',
    })).toBeNull()
  })

  it('uses the label in server archive names', () => {
    expect(serverZipBaseName('linux-x64', {})).toBe('pointer-server-community-linux-x64')
    expect(serverDebBaseName('0.1.2', 'amd64', { POINTER_PACKAGE_LABEL: 'official' })).toBe(
      'pointer-server-official-0.1.2-amd64',
    )
  })
})

describe('markBuiltReleaseFiles', () => {
  it('renames the installer where the packager wrote it', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-mark-'))
    const debDir = path.join(root, 'target', 'release', 'bundle', 'deb')
    fs.mkdirSync(debDir, { recursive: true })
    fs.writeFileSync(path.join(debDir, 'Pointer_0.1.2_amd64.deb'), 'client')

    const renamed = markBuiltReleaseFiles(root, { POINTER_PACKAGE_LABEL: 'community' })

    expect(renamed.map((file) => path.basename(file))).toEqual(['Pointer-community-0.1.2-amd64.deb'])
    expect(fs.existsSync(path.join(debDir, 'Pointer-community-0.1.2-amd64.deb'))).toBe(true)
  })
})

describe('stageReleaseAssets', () => {
  it('copies client and server artifacts from the build trees', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-release-'))
    const debDir = path.join(root, 'target', 'release', 'bundle', 'deb')
    const serverDir = path.join(root, 'target', 'release', 'pointer-server-bundle')
    fs.mkdirSync(debDir, { recursive: true })
    fs.mkdirSync(serverDir, { recursive: true })
    fs.writeFileSync(path.join(debDir, 'Pointer_0.1.2_amd64.deb'), 'client')
    fs.writeFileSync(path.join(serverDir, 'pointer-server-linux-x64.zip'), 'server')

    const dest = path.join(root, 'release-assets')
    const copied = stageReleaseAssets({ root, flavor: 'community', dest })

    expect(copied.map((file) => path.basename(file)).sort()).toEqual([
      'Pointer-community-0.1.2-amd64.deb',
      'pointer-server-community-linux-x64.zip',
    ])
    expect(fs.readFileSync(path.join(dest, 'Pointer-community-0.1.2-amd64.deb'), 'utf8')).toBe(
      'client',
    )
  })

  it('ignores bundle-internal assets such as the AppImage icon', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-release-'))
    const appimageDir = path.join(root, 'target', 'release', 'bundle', 'appimage')
    fs.mkdirSync(appimageDir, { recursive: true })
    fs.writeFileSync(path.join(appimageDir, 'Pointer_0.1.2_amd64.AppImage'), 'client')
    fs.writeFileSync(path.join(appimageDir, 'Pointer.png'), 'icon')

    const dest = path.join(root, 'release-assets')
    const copied = stageReleaseAssets({ root, flavor: 'community', dest })

    expect(copied.map((file) => path.basename(file))).toEqual([
      'Pointer-community-0.1.2-amd64.AppImage',
    ])
  })
})
