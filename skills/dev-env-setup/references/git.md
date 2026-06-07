# Git 安装指南

## Windows

### 方案一：华为镜像下载安装（推荐，国内首选）

1. 查看可用版本：打开 https://mirrors.huaweicloud.com/git-for-windows/ 确认最新版本号
2. 下载（PowerShell）：
```powershell
$url = "https://mirrors.huaweicloud.com/git-for-windows/v2.54.0.windows.1/Git-2.54.0-64-bit.exe"
$out = "$env:TEMP\Git-2.54.0-64-bit.exe"
Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 300 -OutFile $out
```
3. 静默安装：
```powershell
Start-Process -Wait -FilePath "$env:TEMP\Git-2.54.0-64-bit.exe" `
    -ArgumentList "/VERYSILENT /NORESTART /NOCANCEL /SP-"
```
4. 验证：
```powershell
# 当前进程添加 PATH
$env:Path = "C:\Program Files\Git\bin;$env:Path"
git --version
```
5. 确认安装位置：
```powershell
Get-ChildItem "$env:ProgramFiles\Git\bin\git.exe"
```

### 方案二：安装包手动安装（备选）

1. 从 https://git-scm.com/downloads/win 下载
2. 双击运行，按向导完成安装
3. 勾选 "Git from the command line and also from 3rd-party software"

> **提示**：安装完成后可能需要重启终端以使 PATH 生效。

---

## macOS

### 方案一：Homebrew

```bash
brew install git
git --version
```

### 方案二：Xcode Command Line Tools

```bash
xcode-select --install
git --version
```

---

## Linux

### Debian / Ubuntu

```bash
sudo apt update
sudo apt install git -y
git --version
```

### Fedora / RHEL / CentOS

```bash
sudo dnf install git -y
git --version
```

### Arch Linux

```bash
sudo pacman -S git
git --version
```

---

## 基本配置（全平台通用）

```bash
git config --global user.name "Your Name"
git config --global user.email "your.email@example.com"
```

### 国内代理加速（可选）

```bash
# 配置 HTTP 代理
git config --global http.proxy http://127.0.0.1:7890
git config --global https.proxy http://127.0.0.1:7890

# 取消代理
git config --global --unset http.proxy
git config --global --unset https.proxy
```

### 用国内镜像加速 clone

```bash
# 原始地址
git clone https://github.com/user/repo.git
# 替换为华为云镜像
git clone https://mirrors.huaweicloud.com/github/user/repo.git
```

### 验证

```bash
git config --list
git --version
```
