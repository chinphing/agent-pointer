/** OS-arch tag for release bundle names, e.g. macos-arm64, linux-x64, windows-x64. */
export function platformTag() {
  const osName =
    process.platform === 'darwin'
      ? 'macos'
      : process.platform === 'win32'
        ? 'windows'
        : process.platform
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
  return `${osName}-${arch}`
}

export function platformLabel() {
  const osName =
    process.platform === 'darwin'
      ? 'macOS'
      : process.platform === 'win32'
        ? 'Windows'
        : process.platform === 'linux'
          ? 'Linux'
          : process.platform
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
  return `${osName} ${arch}`
}
