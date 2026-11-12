import { describe, expect, it } from 'vitest'
import { classifyReleaseFile, isAcceptedForPlatform, signatureTarget } from './releaseFiles'

describe('classifyReleaseFile', () => {
  it('安装包按扩展名归位', () => {
    expect(classifyReleaseFile('Pointer_1.0.0_universal.dmg', 'macos')).toBe('macos_dmg')
    expect(classifyReleaseFile('Pointer_1.0.0_x64_en-US.msi', 'windows')).toBe('windows_msi')
    // NSIS 安装包：改名后是 `Pointer-official-1.0.0-x64-setup.exe`
    expect(classifyReleaseFile('Pointer-official-1.0.0-x64-setup.exe', 'windows')).toBe('windows_nsis')
    // NSIS 更新包是 `.nsis.zip`，必须归到 updater，不能当成安装包
    expect(classifyReleaseFile('Pointer-official-1.0.0-x64.nsis.zip', 'windows')).toBe('windows_updater')
    expect(classifyReleaseFile('pointer_1.0.0_amd64.deb', 'linux')).toBe('linux_deb')
    expect(classifyReleaseFile('pointer_1.0.0_amd64.AppImage', 'linux')).toBe('linux_appimage')
  })

  it('压缩包一律算更新包，且归属跟随平台', () => {
    expect(classifyReleaseFile('Pointer.app.tar.gz', 'macos')).toBe('macos_updater')
    expect(classifyReleaseFile('Pointer_1.0.0_x64-setup.nsis.zip', 'windows')).toBe(
      'windows_updater'
    )
  })

  // 回归：`.AppImage.tar.gz` 同时像压缩包又像 AppImage，
  // 判错就会把更新包当安装包发给用户。
  it('AppImage 更新包不能被判成 AppImage 安装包', () => {
    expect(classifyReleaseFile('pointer_1.0.0_amd64.AppImage.tar.gz', 'linux')).toBe(
      'linux_appimage_updater'
    )
    expect(classifyReleaseFile('pointer_1.0.0_amd64.AppImage', 'linux')).toBe('linux_appimage')
  })

  it('大小写不敏感', () => {
    expect(classifyReleaseFile('POINTER.DMG', 'macos')).toBe('macos_dmg')
    expect(classifyReleaseFile('Pointer.AppImage', 'linux')).toBe('linux_appimage')
  })

  it('认不出来的返回 null', () => {
    expect(classifyReleaseFile('README.md', 'macos')).toBeNull()
    expect(classifyReleaseFile('Pointer.dmg.sig', 'macos')).toBeNull()
    expect(classifyReleaseFile('', 'linux')).toBeNull()
  })
})

describe('isAcceptedForPlatform', () => {
  it('跨平台的文件会被拒绝', () => {
    expect(isAcceptedForPlatform('Pointer.msi', 'macos')).toBe(false)
    expect(isAcceptedForPlatform('Pointer.dmg', 'macos')).toBe(true)
  })

  it('Linux 三个类型都收', () => {
    expect(isAcceptedForPlatform('a.deb', 'linux')).toBe(true)
    expect(isAcceptedForPlatform('a.AppImage', 'linux')).toBe(true)
    expect(isAcceptedForPlatform('a.AppImage.tar.gz', 'linux')).toBe(true)
  })
})

describe('signatureTarget', () => {
  it('去掉 .sig 得到归档名', () => {
    expect(signatureTarget('Pointer.app.tar.gz.sig')).toBe('Pointer.app.tar.gz')
    expect(signatureTarget('Pointer.dmg')).toBeNull()
  })
})
