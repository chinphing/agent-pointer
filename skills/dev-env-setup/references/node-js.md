# Node.js / TypeScript 安装指南

## macOS / Linux

### 安装 nvm（推荐）

```bash
export NVM_NODEJS_ORG_MIRROR=https://mirrors.huaweicloud.com/nodejs
curl -o- https://gitee.com/mirrors/nvm/raw/master/install.sh | bash
source ~/.zshrc  # 或 ~/.bashrc
```

### 安装 Node.js LTS

```bash
nvm install --lts
nvm use --lts
node --version
npm --version
```

### 配置 npm 华为镜像

```bash
npm config set registry https://mirrors.huaweicloud.com/repository/npm/
```

### 验证

```bash
node -e "console.log('Node.js works')"
```

## Windows

> **推荐方案：下载 zip 解压到用户目录**（无需管理员权限，避免 MSI/winget 的下载缓慢和证书问题）

### 1. 查看可用版本

从华为镜像查看最新 LTS 版本号：

```powershell
Invoke-WebRequest -Uri "https://mirrors.huaweicloud.com/nodejs/" -UseBasicParsing -TimeoutSec 15 -OutFile "$env:TEMP\node_ver.html"
Get-Content "$env:TEMP\node_ver.html" | Select-String 'v24\.\d+\.\d+/' | Select-Object -Last 5
```

当前 LTS 系列为 **24.x**（2025-2026 LTS，Active）。建议选最新的 24.x 版本。

### 2. 下载安装包（华为镜像）

```powershell
# 替换为查询到的最新 LTS 版本号
$version = "24.15.0"
$url = "https://mirrors.huaweicloud.com/nodejs/v$version/node-v$version-win-x64.zip"
Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 300 -OutFile "$env:TEMP\node-v$version-win-x64.zip"
```

> ⚠️ **踩坑记录**：v24.16.0 的 zip 包缺失 `npm.cmd`/`npx.cmd` 以及 npm 的 `package.json`，导致 npm 无法使用。如遇到此类问题，降级到上一个版本。

### 3. 解压到用户目录

```powershell
# 使用 .NET ZipFile（比 Expand-Archive 更可靠）
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::ExtractToDirectory(
    "$env:TEMP\node-v$version-win-x64.zip",
    "$env:LOCALAPPDATA\Programs\nodejs"
)
# 注意：解压后文件夹名为 node-v{version}-win-x64
```

目标结构：`%LOCALAPPDATA%\Programs\nodejs\node-v{version}-win-x64\`

### 4. 配置用户级 PATH（无需管理员权限）

```powershell
$nodeBin = "$env:LOCALAPPDATA\Programs\nodejs\node-v$version-win-x64"
# 获取当前用户 PATH，移除旧版 Node.js 路径
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$newPath = @($userPath -split ';' | Where-Object {
    $_ -notlike "*nodejs*" -and $_ -ne ''
}) -join ';'
$newPath = "$nodeBin;$newPath"
[Environment]::SetEnvironmentVariable("Path", $newPath, "User")
```

> 需要**打开新终端**后 `node`/`npm` 才生效。

### 5. 配置 npm 华为镜像

```powershell
npm config set registry https://mirrors.huaweicloud.com/repository/npm/
```

### 6. 验证

```powershell
node --version
npm --version
npx --version
npm config get registry
```

---

## 国内镜像说明

| 资源 | 镜像地址 |
|------|---------|
| npm registry | `https://mirrors.huaweicloud.com/repository/npm/` |
| Node 二进制 | `https://mirrors.huaweicloud.com/nodejs/` |

## 踩坑备忘录

| 问题 | 原因 | 解决方案 |
|------|------|---------|
| npm 报 "Cannot find module '../../package.json'" | 镜像打包的 v24.16.0 缺失 npm package.json | 换用 v24.15.0 或下一个修复版 |
| `npm.cmd` 不存在于根目录 | v24.16.0 的 zip 缺少 npm/npx cmd 文件 | 同上 |
| `msiexec /i` 安装报错或无响应 | 国内网络访问微软 CDN 慢或证书问题 | 改用 zip 解压安装 |
| `corepack` 下载 npm 卡住 | corepack 默认从 npmjs.org 下载（国内慢） | 确保 npm.cmd 存在于根目录，或 `corepack disable npm` |
| `winget install` 超时/证书错误 | winget 源走国外服务器 | 不用 winget，用华为镜像直接下载 |
