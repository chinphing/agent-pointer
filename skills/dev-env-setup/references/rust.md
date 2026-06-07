# Rust 安装指南

## macOS / Linux

### 安装 rustup（使用华为镜像）

```bash
export RUSTUP_DIST_SERVER=https://mirrors.huaweicloud.com/rust-static
export RUSTUP_UPDATE_ROOT=https://mirrors.huaweicloud.com/rust-static/rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

备用镜像：
- 华为: `https://mirrors.huaweicloud.com/rust-static`
- 中科大: `https://mirrors.ustc.edu.cn/rust-static`

### 配置 cargo 华为镜像

创建 `~/.cargo/config.toml`：

```toml
[source.crates-io]
replace-with = "huawei"

[source.huawei]
registry = "sparse+https://mirrors.huaweicloud.com/repository/crates.io-index/"
```

## Windows

### 下载 rustup-init

```powershell
# 从华为镜像下载 rustup-init
$url = "https://mirrors.huaweicloud.com/rust-static/rustup/archive/1.27.1/x86_64-pc-windows-msvc/rustup-init.exe"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\rustup-init.exe"

# 设置环境变量后运行
$env:RUSTUP_DIST_SERVER = "https://mirrors.huaweicloud.com/rust-static"
$env:RUSTUP_UPDATE_ROOT = "https://mirrors.huaweicloud.com/rust-static/rustup"
Start-Process -FilePath "$env:TEMP\rustup-init.exe" -Wait
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/rust-static/rustup/archive/1.27.1/x86_64-pc-windows-msvc/`
2. 下载 `rustup-init.exe` 并运行（默认选项即可）

### cargo 华为镜像

同上创建 `%USERPROFILE%\.cargo\config.toml`。

### Windows 编译依赖

Rust 在 Windows 需要 **MSVC Build Tools**，从华为镜像下载 VS Build Tools：

```powershell
$url = "https://mirrors.huaweicloud.com/visualstudio/2017/BuildTools/vs_BuildTools.exe"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\vs_BuildTools.exe"
Start-Process -FilePath "$env:TEMP\vs_BuildTools.exe" -ArgumentList "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet" -Wait
```

### 验证

```bash
rustc --version
cargo --version
```
