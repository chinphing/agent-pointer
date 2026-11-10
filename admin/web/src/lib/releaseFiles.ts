/**
 * 发版页的文件归类：一次选中整个平台的产物，按文件名判断各自属于哪个产物类型。
 *
 * 为什么要单独抽出来：`.AppImage.tar.gz` 是**更新包**，`.AppImage` 才是安装包，
 * 只靠 `endsWith('.appimage')` 会把两者搞混。这段逻辑错了，下载页就会给用户
 * 发一个装不上的压缩包。
 */
export type PlatformId = 'macos' | 'windows' | 'linux'

/** 每个平台允许出现的产物类型。 */
export const PLATFORM_KINDS: Record<PlatformId, string[]> = {
  macos: ['macos_dmg', 'macos_updater'],
  windows: ['windows_msi', 'windows_updater'],
  linux: ['linux_deb', 'linux_appimage', 'linux_appimage_updater'],
}

/**
 * 文件名 → 产物类型。平台决定更新包归属，扩展名决定安装包格式。
 * 认不出来的返回 null。
 */
export function classifyReleaseFile(name: string, platform: PlatformId): string | null {
  const n = name.toLowerCase()
  // 先判压缩包：`.AppImage.tar.gz` 是更新包，不能落到 AppImage 安装包上
  if (n.endsWith('.tar.gz') || n.endsWith('.zip')) {
    if (platform === 'macos') return 'macos_updater'
    if (platform === 'windows') return 'windows_updater'
    return 'linux_appimage_updater'
  }
  if (n.endsWith('.dmg')) return 'macos_dmg'
  if (n.endsWith('.msi')) return 'windows_msi'
  if (n.endsWith('.deb')) return 'linux_deb'
  if (n.endsWith('.appimage')) return 'linux_appimage'
  return null
}

/** 这个文件属于该平台、且是能上传的类型。 */
export function isAcceptedForPlatform(name: string, platform: PlatformId): boolean {
  const kind = classifyReleaseFile(name, platform)
  return kind !== null && PLATFORM_KINDS[platform].includes(kind)
}

/** `.sig` 按「归档名 + .sig」配对，返回去掉后缀的归档名。 */
export function signatureTarget(name: string): string | null {
  return name.toLowerCase().endsWith('.sig') ? name.slice(0, -4) : null
}
