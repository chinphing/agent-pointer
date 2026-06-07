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

### 下载安装包

```powershell
# 从华为镜像下载 LTS 版本
$version = "22.14.0"  # 最新 LTS，可前往 https://mirrors.huaweicloud.com/nodejs/ 查看
$arch = "x64"
if ([Environment]::Is64BitOperatingSystem) { $arch = "x64" } else { $arch = "x86" }
$url = "https://mirrors.huaweicloud.com/nodejs/v$version/node-v$version-win-$arch.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\node.zip"
Expand-Archive -Path "$env:TEMP\node.zip" -DestinationPath "C:\nodejs" -Force
```

### 配置环境变量

```powershell
# 添加到 PATH
[Environment]::SetEnvironmentVariable("Path", "$env:Path;C:\nodejs", "User")
# 配置 npm 镜像
npm config set registry https://mirrors.huaweicloud.com/repository/npm/
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/nodejs/` 下载 `node-v22.14.0-win-x64.zip`
2. 解压到 `C:\nodejs`
3. 系统设置 → 环境变量 → 将 `C:\nodejs` 添加到 PATH

### nvm-windows（推荐多版本管理）

下载地址：https://gitee.com/mirrors/nvm-windows

## 国内镜像说明

- npm registry: `https://mirrors.huaweicloud.com/repository/npm/`
- Node 二进制: `https://mirrors.huaweicloud.com/nodejs/`
