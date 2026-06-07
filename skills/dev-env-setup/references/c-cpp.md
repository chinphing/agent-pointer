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

### 方案 A：MSYS2 + MinGW-w64（推荐，完整工具链 + 包管理器）

MSYS2 提供完整的 MinGW-w64 GCC 工具链，含包管理器 pacman 方便后续扩展。

#### 1. 下载 MSYS2（清华镜像）

```powershell
$url = "https://mirrors.tuna.tsinghua.edu.cn/msys2/distrib/msys2-x86_64-latest.sfx.exe"
$out = "$env:TEMP\msys2.exe"
Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
```

#### 2. 安装到 C:\msys64

```powershell
# 解压（7z SFX 格式）
cmd.exe /c "`"$env:TEMP\msys2.exe`" x -oC:\msys64 -y"
```

#### 3. 配置清华镜像源

```powershell
@"
Server = https://mirrors.tuna.tsinghua.edu.cn/msys2/mingw/x86_64
Server = https://mirrors.ustc.edu.cn/msys2/mingw/x86_64
"@ | Out-File -FilePath "C:\msys64\etc\pacman.d\mirrorlist.mingw64" -Encoding utf8

@"
Server = https://mirrors.tuna.tsinghua.edu.cn/msys2/msys/`$arch
Server = https://mirrors.ustc.edu.cn/msys2/msys/`$arch
"@ | Out-File -FilePath "C:\msys64\etc\pacman.d\mirrorlist.msys" -Encoding utf8
```

#### 4. 初始化 MSYS2

```powershell
# 自动初始化密钥环
C:\msys64\msys2_shell.cmd -defterm -here -no-start -c "pacman-key --init 2>&1"
```

#### 5. 安装 MinGW-w64 GCC 工具链

```powershell
# 系统更新 + 安装 GCC/G++/GDB/Make/CMake
C:\msys64\usr\bin\bash.exe -l -c "pacman -Syu --noconfirm 2>&1"
C:\msys64\usr\bin\bash.exe -l -c "pacman -Suu --noconfirm 2>&1"
C:\msys64\usr\bin\bash.exe -l -c "pacman -S --noconfirm --needed mingw-w64-x86_64-gcc mingw-w64-x86_64-gdb mingw-w64-x86_64-make mingw-w64-x86_64-cmake mingw-w64-x86_64-binutils 2>&1"
```

#### 6. 添加系统 PATH

```powershell
$mingwBin = "C:\msys64\mingw64\bin"
[Environment]::SetEnvironmentVariable(
    "PATH",
    "$mingwBin;$([Environment]::GetEnvironmentVariable('PATH', 'Machine'))",
    "Machine"
)
```

#### 7. 验证

```powershell
# 重启终端后执行
gcc --version
g++ --version
cmake --version
gdb --version
mingw32-make --version

# 快速测试
gcc test.c -o test.exe && test.exe
```

### 方案 B：MinGW-w64 直装版（轻量，适合快速开始）

```powershell
# 从 SourceForge 镜像下载
$url = "https://github.com/brechtsanders/winlibs_mingw/releases/download/14.2.0posix-22.1-12.0.0-msvcrt-r1/winlibs-x86_64-posix-seh-gcc-14.2.0-mingw-w64msvcrt-12.0.0-r1.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\mingw.zip"
Expand-Archive -Path "$env:TEMP\mingw.zip" -DestinationPath "C:\mingw64" -Force

# 添加 PATH
$mingwPath = "C:\mingw64\mingw64\bin"
[Environment]::SetEnvironmentVariable("PATH", "$mingwPath;$env:PATH", "User")
```

### 方案 C：Visual Studio + MSVC（完整方案，适合大型项目）

1. 下载 Visual Studio Community：https://visualstudio.microsoft.com/
2. 安装时勾选 "使用 C++ 的桌面开发"
3. MSVC 编译器将随 Visual Studio 自动配置

## 验证（通用）

```bash
gcc --version
g++ --version
cmake --version
gdb --version
```

## 快速测试

创建 `hello.c`：

```c
#include <stdio.h>
int main() {
    printf("Hello, C!\n");
    return 0;
}
```

创建 `hello.cpp`：

```cpp
#include <iostream>
int main() {
    std::cout << "Hello, C++!" << std::endl;
    return 0;
}
```

编译运行：

```bash
gcc hello.c -o hello.exe && hello.exe
g++ hello.cpp -o hello.exe && hello.exe
```

## 注意事项

1. **MSYS2 命令行**：用 `bash.exe -l -c "命令"` 或 `msys2_shell.cmd -defterm -here -no-start -c "命令"` 在 MSYS2 环境中执行命令
2. **Make 命名**：Windows 上 make 是 `mingw32-make.exe`（非 `make`）
3. **系统 PATH**：添加 `C:\msys64\mingw64\bin` 后可直接在 CMD/PowerShell 中使用 gcc/g++ 等命令
4. **MSYS2 包管理**：通过 `pacman -S mingw-w64-x86_64-xxx` 安装更多工具（如 ninja、boost、openssl 等）
5. **MSYS2 中文支持**：MSYS2 终端默认 UTF-8，控制台输出中文正常
