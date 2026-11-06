import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { releaseAssetName, stageReleaseAssets } from './rename-release-assets.mjs'

describe('releaseAssetName', () => {
  it('marks client installers and updater signatures', () => {
    expect(releaseAssetName('Pointer_0.1.2_amd64.deb', 'community')).toBe(
      'Pointer_community_0.1.2_amd64.deb',
    )
    expect(releaseAssetName('Pointer_0.1.2_x64-setup.exe', 'official')).toBe(
      'Pointer_official_0.1.2_x64-setup.exe',
    )
    expect(releaseAssetName('Pointer_0.1.2_x64_zh-CN.msi', 'community')).toBe(
      'Pointer_community_0.1.2_x64_zh-CN.msi',
    )
    expect(releaseAssetName('Pointer_0.1.2_amd64.AppImage', 'official')).toBe(
      'Pointer_official_0.1.2_amd64.AppImage',
    )
    expect(releaseAssetName('Pointer_0.1.2_amd64.deb.sig', 'official')).toBe(
      'Pointer_official_0.1.2_amd64.deb.sig',
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
      'pointer-server-official_0.1.2_amd64.deb',
    )
  })

  it('leaves an already marked name unchanged', () => {
    expect(releaseAssetName('Pointer_community_0.1.2_amd64.deb', 'community')).toBe(
      'Pointer_community_0.1.2_amd64.deb',
    )
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
      'Pointer_community_0.1.2_amd64.deb',
      'pointer-server-community-linux-x64.zip',
    ])
    expect(fs.readFileSync(path.join(dest, 'Pointer_community_0.1.2_amd64.deb'), 'utf8')).toBe(
      'client',
    )
  })
})
