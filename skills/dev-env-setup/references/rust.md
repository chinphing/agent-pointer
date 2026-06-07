# Rust 安装指南

## 镜像源说明

所有链接替换为国内镜像即可加速。推荐以下镜像源：

| 镜像源 | RUSTUP_DIST_SERVER | cargo 镜像 |
|--------|---------------------|-------------|
| **清华 (TUNA)** | `https://mirrors.tuna.tsinghua.edu.cn/rustup` | `sparse+https://mirrors.tuna.tsinghua.edu.cn/git/crates.io-index/` |
| **中科大 (USTC)** | `https://mirrors.ustc.edu.cn/rust-static` | 暂不支持 sparse 协议 |
| **华为云** | `https://mirrors.huaweicloud.com/rust-static` | `sparse+https://mirrors.huaweicloud.com/repository/crates.io-index/` |
| **阿里云** | `https://mirrors.aliyun.com/rustup` | 暂不支持 sparse 协议 |
| **上海交大 (SJTUG)** | `https://mirrors.sjtug.sjtu.edu.cn/rust-static` | 暂不支持 sparse 协议 |

> 注：cargo sparse 协议（v1.68+）比传统 git 索引更快，推荐使用支持 sparse 的镜像。

---

## macOS / Linux

### 一键安装（使用清华镜像）

```bash
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 配置 cargo 清华镜像

```bash
mkdir -p ~/.cargo
cat > ~/.cargo/config.toml << 'EOF'
[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/git/crates.io-index/"
EOF
```

### 永久配置镜像环境变量（可选）

写入 shell 配置文件（如 `~/.bashrc` 或 `~/.zshrc`）：

```bash
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
```

---

## Windows

### 方法一：下载安装包 + 镜像环境变量（推荐）

```powershell
# 1. 从清华镜像下载 rustup-init.exe
curl -o "$env:TEMP\rustup-init.exe" "https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"

# 2. 设置镜像环境变量后再安装
$env:RUSTUP_DIST_SERVER = "https://mirrors.tuna.tsinghua.edu.cn/rustup"
$env:RUSTUP_UPDATE_ROOT = "https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup"
& "$env:TEMP\rustup-init.exe" -y

# 3. 永久设置镜像环境变量（后续 rustup update 也走镜像）
[System.Environment]::SetEnvironmentVariable("RUSTUP_DIST_SERVER", "https://mirrors.tuna.tsinghua.edu.cn/rustup", "User")
[System.Environment]::SetEnvironmentVariable("RUSTUP_UPDATE_ROOT", "https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup", "User")

# 4. 配置 cargo 清华 sparse 镜像
$cargoConfig = "$env:USERPROFILE\.cargo\config.toml"
if (-not (Test-Path (Split-Path $cargoConfig))) {
    New-Item -ItemType Directory -Path (Split-Path $cargoConfig) -Force | Out-Null
}
@"
[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/git/crates.io-index/"
"@ | Out-File -FilePath $cargoConfig -Encoding utf8
```

### 方法二：各镜像源直接下载链接

64 位 MSVC 版本（大多数 Windows 用户选择）：

| 镜像源 | 下载地址 |
|--------|----------|
| **清华** | `https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe` |
| **中科大** | `https://mirrors.ustc.edu.cn/rust-static/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe` |
| **华为云** | `https://mirrors.huaweicloud.com/rust-static/rustup/archive/1.27.1/x86_64-pc-windows-msvc/rustup-init.exe` |
| **阿里云** | `https://mirrors.aliyun.com/rustup/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe` |
| **上海交大** | `https://mirrors.sjtug.sjtu.edu.cn/rust-static/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe` |

下载后运行：

```powershell
$env:RUSTUP_DIST_SERVER = "https://mirrors.tuna.tsinghua.edu.cn/rustup"
$env:RUSTUP_UPDATE_ROOT = "https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup"
.\rustup-init.exe
```

### 验证

```powershell
# 如提示找不到命令，先刷新 PATH
$env:Path = [System.Environment]::GetEnvironmentVariable("Path","User") + ";" + [System.Environment]::GetEnvironmentVariable("Path","Machine")

rustc --version
cargo --version
```

---

## 常见问题

### 安装时卡在 "downloading" 不动

原因：官方下载地址被墙或极慢。解决方法：
1. 退出安装程序
2. 设置 `RUSTUP_DIST_SERVER` 和 `RUSTUP_UPDATE_ROOT` 为国内镜像
3. 重新运行安装程序

### 安装完 rustc 找不到命令

Windows 下需要重新打开终端或手动刷新 `PATH`：

```powershell
$env:Path = [System.Environment]::GetEnvironmentVariable("Path","User") + ";" + [System.Environment]::GetEnvironmentVariable("Path","Machine")
```

### cargo build 下载依赖慢

检查 `~/.cargo/config.toml` 是否配置了镜像源。新装 Rust 的用户容易漏掉这一步。
