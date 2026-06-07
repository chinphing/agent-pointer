# C/C++ 开发环境安装指南

## macOS

### Xcode Command Line Tools

```bash
xcode-select --install
```

验证：
```bash
clang --version
make --version
```

> macOS 的 `gcc` 实为 `clang`。如需真正 GCC，用 Homebrew 安装：
> `brew install gcc`

## Linux

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install build-essential cmake gdb

# CentOS/RHEL/Fedora
sudo dnf groupinstall "Development Tools"
sudo dnf install cmake gdb
```

## Windows

### 方案 A：MinGW-w64（轻量级，适合 C/C++ 学习）

```powershell
# 从华为镜像下载 MinGW-w64
$url = "https://mirrors.huaweicloud.com/mingw/MinGW-W64-Installer/mingw-w64-install.exe"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\mingw-install.exe"
Start-Process -FilePath "$env:TEMP\mingw-install.exe" -Wait
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/mingw/MinGW-W64-Installer/`
2. 下载 `mingw-w64-install.exe`
3. 安装时选 **x86_64** 架构
4. 将 `C:\mingw-w64\x86_64-8.1.0-posix-seh-rt_v6-rev0\mingw64\bin` 添加到系统 PATH

### 方案 B：MSVC（完整 VC++ 工具链，Rust/Python 编译依赖）

```powershell
# 从华为镜像下载 VS Build Tools
$url = "https://mirrors.huaweicloud.com/visualstudio/2022/BuildTools/vs_BuildTools.exe"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\vs_BuildTools.exe"
Start-Process -FilePath "$env:TEMP\vs_BuildTools.exe" -ArgumentList "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet" -Wait
```

### 验证

```bash
gcc --version      # MinGW
g++ --version
```
