# 跨平台开发与打包

本文档汇总 **Windows / macOS / Linux** 上的环境准备、本地开发、编译打包与 CI 发版流程。  
工作目录均为仓库内的 **`pointer-app/`**（Tauri + Vue 项目根）。

---

## 架构概览

```text
Vue 统一界面
├─ 桌面端：Tauri Adapter → src-tauri → crates/pointer-core
└─ Web 端：Web Adapter → server(axum HTTP/SSE) → crates/pointer-core
```

| 能力 | 桌面端 | Web 端 |
|------|--------|--------|
| 启动命令 | `npm run tauri:dev` | `npm run server:dev` + `npm run dev:web` |
| 打包 | `npm run tauri:build` | 前端 `npm run build`，后端 `cargo build -p pointer-server --release` |
| 电脑操控 | 完整（需各平台权限/依赖） | 受限（无本地截图/输入） |

---

## 通用依赖

所有平台均需：

| 工具 | 版本建议 | 检查命令 |
|------|----------|----------|
| Node.js | 20+ | `node -v` |
| npm | 随 Node | `npm -v` |
| Rust | stable | `rustc --version` / `cargo --version` |

Tauri CLI 由项目 devDependency 提供，**不要依赖全局 `tauri` 命令**，统一使用 `npm run tauri:*`。

`pointer-core` 在编译时会构建 **sqlite-cjk-fts** 原生扩展（`cjk_bigram` FTS5 分词器），用于 `session_search` 中文检索。需本机 C 编译器：

| 平台 | 要求 |
|------|------|
| macOS / Linux | `cc` 或 `gcc`（Xcode CLT / build-essential） |
| Windows | MSVC `cl` 或 MinGW `gcc` |

扩展在 build 时**静态链接**进 `pointer-core`，通过 `sqlite3_auto_extension` 注册，无需运行时释放 DLL。

**对话历史**存于 `{data_dir}/PointerApp/conversations.db`（SQLite WAL，Hermes 同型写重试 + 增量 upsert）。首次升级会从 `conversations.json` 自动导入。旧版 `sessions.db` 索引库已废弃。

首次进入项目：

```bash
cd pointer-app
npm install
```

---

## Windows

### 环境要求

- **Microsoft C++ Build Tools**（勾选「使用 C++ 的桌面开发」）
- **LLVM（libclang）** — 仅**从源码编译**时需要；`silk-v3-sys` 的 build 脚本通过 bindgen 生成 C 绑定。最终用户安装 MSI 后**不需要** LLVM。见下文「安装 LLVM / libclang」。
- **WebView2 Runtime**（Win10/11 通常已预装；开发机缺失时从微软官网安装 Evergreen Bootstrapper）

### 安装 LLVM / libclang（开发者）

bindgen 在 Windows 上需要 `libclang.dll`（或 `clang.dll`）。任选一种安装方式，**安装后新开终端**。

**方式 1：winget（推荐）**

```powershell
winget install LLVM.LLVM
```

默认路径：`C:\Program Files\LLVM\bin\libclang.dll`

**方式 2：官方安装包**

从 [LLVM Releases](https://github.com/llvm/llvm-project/releases) 下载 **Windows 64-bit** 安装程序（例如 `LLVM-19.x.x-win64.exe`），安装时勾选 **Add LLVM to the system PATH**。

**配置环境变量**

若安装后仍报 `Unable to find libclang`，在 PowerShell 中设置（路径按实际安装目录调整）：

```powershell
# 当前终端会话
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"

# 永久写入用户环境变量（执行后新开终端）
[Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")
```

**验证**

```powershell
Test-Path "C:\Program Files\LLVM\bin\libclang.dll"   # 应输出 True
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
npm run tauri:build
```

若 LLVM 装在 Visual Studio 的 Clang 组件下，常见路径为：

`C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin`

### 开发

```powershell
cd pointer-app
npm install
npm run tauri:dev
```

打开 DevTools：`Ctrl + Shift + I`

设置 Web 前端 API 地址（PowerShell）：

```powershell
$env:VITE_WEB_API_BASE="http://127.0.0.1:8787"; npm run dev:web
```

### 打包

```powershell
npm run icons          # 首次或更换 icon.png 后
npm run build:windows  # 与 npm run tauri:build 相同（Windows 不设 NO_STRIP）
```

**产物目录：**

```text
src-tauri/target/release/bundle/
└── msi/               # *.msi 安装程序
```

**Release 二进制：**

```text
src-tauri/target/release/pointer-app.exe
```

### 常见问题

| 现象 | 处理 |
|------|------|
| 提示缺少 C++ 工具链 | 安装 [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选 **「使用 C++ 的桌面开发」**；安装后**新开**终端再编译 |
| `failed to run C compiler cl` / `cjk fts` 编译失败 | 同上；或在已配置 MSVC 的 **x64 Native Tools Command Prompt** 中执行 `npm run tauri:build`。MinGW 工具链可设置 `CC=gcc` |
| `Unable to find libclang`（`silk-v3-sys` / bindgen） | 按上文「安装 LLVM / libclang」安装 LLVM，设置 `LIBCLANG_PATH` 为含 `libclang.dll` 的 `bin` 目录，**新开终端**后重编 |
| `tauri` 找不到 | 使用 `npm run tauri:dev`，先 `npm install` |
| 电脑操控无响应 | 检查屏幕录制/辅助功能权限；以 Release 包测试 |

### 正式发布（可选）

- 配置 **Authenticode 代码签名**，避免 SmartScreen 拦截
- 应用数据目录：`%APPDATA%\PointerApp\`

---

## macOS

### 环境要求

```bash
xcode-select --install
```

需 **Node.js 20+**、**Rust stable**。打包 `.app` / `.dmg` **必须在 macOS 上构建**（无法交叉编译出可分发 macOS 包）。

**最低系统版本：** macOS **10.15（Catalina）**（见 `src-tauri/tauri.conf.json` → `bundle.macOS.minimumSystemVersion`）。

### 开发

```bash
cd pointer-app
npm install
npm run tauri:dev
```

打开 DevTools：`Cmd + Option + I`

### 打包

```bash
npm run icons
npm run build:macos              # 不签名（默认）
npm run build:macos:sign-only    # 仅签名，不公证（需 signing.env 中的 p12 配置）
npm run build:macos:signed       # Developer ID 签名 + 公证（需完整 signing.env）
npm run build:macos:universal    # Universal，不签名
```

**签名 + 公证（正式发布）：**

```bash
# 交互式生成 signing/macos/signing.env（p12 路径、密码、Team ID、Apple ID 等）
npm run signing:macos:setup

# 打包（Universal Binary，与 CI 一致）
npm run build:macos:signed
```

详见 [`signing/macos/README.md`](../../signing/macos/README.md)。  
`signing.env`、`*.p12` 已 gitignore，勿提交仓库。

**通用二进制（Intel + Apple Silicon，不签名）：**

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run build:macos:universal
```

**产物目录：**

```text
src-tauri/target/release/bundle/
├── macos/             # *.app
└── dmg/               # *.dmg
```

### 电脑操控权限

macOS 电脑操控需「屏幕录制 + 辅助功能」。  
`tauri dev` 下可执行文件可能不在 `.app` 内，权限向导可能不完整——**建议用打包后的 `.app` 测试**。  
详见 [`docs/internals/macos-computer-permissions.md`](../internals/macos-computer-permissions.md)。

### 打开提示「已损坏，无法打开」

从浏览器 / 网盘 / 聊天工具下载的 `.dmg` / `.app`，若 **未做 Developer ID 签名 + 公证**，macOS Gatekeeper 常会显示「已损坏」或「无法验证开发者」——**不是安装包真的坏了**，而是系统拒绝运行未信任来源的应用。

**用户侧（内测 / 自构建包）：**

1. **推荐**：不要双击 DMG 里直接打开的 App；将 `Pointer.app` 拖入「应用程序」后，在终端执行（路径按实际修改）：
   ```bash
   xattr -cr /Applications/Pointer.app
   ```
   去掉下载隔离属性 `com.apple.quarantine` 后再从启动台或应用程序文件夹打开。

2. **或**：Finder 中 **右键** `Pointer.app` → **打开** → 在对话框中再次点 **打开**（仅首次需要）。

3. 若仍失败，在「系统设置 → 隐私与安全性」中查看是否有「仍要打开」按钮。

**分发侧（正式发布）：** 见下文「签名与分发」——需 Apple Developer 证书签名并完成 Notarization，用户才无需上述步骤。

### 正式发布（可选）

- Apple Developer 证书签名
- **Notarization（公证）** 后再分发
- 应用数据：`~/Library/Application Support/PointerApp/`

---

## Linux（Ubuntu / Debian）

CI 使用 **ubuntu-22.04**；Ubuntu 24.04 同样适用下列依赖。

### 系统依赖

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config
# 或一次性安装 Tauri Linux 打包全套依赖：
# bash scripts/install-linux-build-deps.sh
```

完整 Tauri 打包还需 WebKitGTK、FUSE、GStreamer 等，见 `scripts/install-linux-build-deps.sh` 或下文「打包」一节。

仅编译 Rust 原生依赖（如 `xcap`）时，可再装：

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libpipewire-0.3-dev \
  libspa-0.2-dev \
  libclang-dev \
  libgbm-dev \
  libegl1-mesa-dev \
  libdrm-dev \
  libwayland-dev \
  build-essential \
  ca-certificates \
  pkg-config \
  libxdo-dev \
  librsvg2-dev \
  libgdk-pixbuf-2.0-dev \
  libgtk-3-dev \
  libgtk-3-bin \
  patchelf
```

说明：

- `build-essential`：提供 `gcc`/`cc` 链接器；缺了会报 **`linker cc not found`**
- `libwebkit2gtk-4.1-dev`、`libglib2.0-dev`：Tauri 桌面壳依赖 GTK/WebKit；缺了会报 **`glib-sys` / `Package 'glib-2.0' not found`**
- `libpipewire-0.3-dev`、`libspa-0.2-dev`：Linux 截图库 `xcap` 编译所需（缺了会报 `libspa-sys` / `libpipewire-0.3` not found）
- `libclang-dev`：`libspa-sys` 通过 bindgen 生成 C 绑定时需要 `libclang.so`（缺了会报 `Unable to find libclang`）
- `libxdo-dev`：电脑操控 agent（`enigo`）链接 `libxdo`；缺了会报 **`unable to find library -lxdo`**
- Wayland 下截图/输入能力因 compositor 而异，复杂场景建议 X11 会话验证

**Windows Computer 截图**：默认走 xcap WGC（D3D11）。在 RDP、虚拟机或 GPU 内存不足（`E_OUTOFMEMORY` / `0x8007000E`）时 WGC 可能 panic；`pointer-core` 会捕获后自动回退 GDI BitBlt（`vision/windows_gdi.rs`）。若两者均失败，请关闭占 GPU 的程序或在本机桌面（非远程会话）重试。

**Fedora / RHEL** 等发行版需自行对照安装 WebKitGTK 4.1、ayatana-appindicator、librsvg 等同名开发包；本文以 Ubuntu 为准。

### 安装 Node.js 与 Rust（若系统未带）

```bash
# Node.js 20
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt-get install -y nodejs

# Rust
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 开发

```bash
cd pointer-app
npm install
npm run tauri:dev
```

打开 DevTools：`Ctrl + Shift + I`

### 打包

```bash
npm run icons
npm run build:linux    # 等价于 npm run tauri:build
```

**产物目录：**

```text
src-tauri/target/release/bundle/
├── deb/               # *.deb
└── appimage/          # *.AppImage
```

**仅打某一种格式：**

```bash
npm run tauri:build -- --bundles deb
npm run tauri:build -- --bundles appimage   # Linux 上 `scripts/tauri-build.mjs` 自动 NO_STRIP=true
# 等价快捷脚本：
npm run build:linux:appimage
```

**安装 deb：**

```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/*.deb
sudo apt-get install -f
```

**运行 AppImage：**

```bash
chmod +x src-tauri/target/release/bundle/appimage/*.AppImage
./src-tauri/target/release/bundle/appimage/*.AppImage
```

**Release 二进制：**

```text
src-tauri/target/release/pointer-app
```

### 常见问题

| 现象 | 处理 |
|------|------|
| `failed to run linuxdeploy`（AppImage 阶段） | ① **deb 已成功时**可先用 `bundle/deb/*.deb`；② 跑 `bash scripts/install-linux-build-deps.sh`（含 `libfuse2`、`squashfs-tools`、`patchelf`、`file`）；③ **必须** `npm run tauri:build`（自动 `NO_STRIP=true` + `APPIMAGE_EXTRACT_AND_RUN=1`），勿 `cargo tauri build`；④ 看详情：`npm run tauri:build -- --bundles appimage --verbose`（常见为 `Strip call failed` / `.relr.dyn` → 确认 `NO_STRIP=true`） |
| `no 'libdir' variable for 'librsvg-2.0'` / gtk plugin exit 1 | 安装 `librsvg2-dev libgdk-pixbuf-2.0-dev libgtk-3-dev libgtk-3-bin`；验证 `pkg-config --variable=libdir librsvg-2.0` 有输出 |
| WebKitGTK 找不到 | 确认 `libwebkit2gtk-4.1-dev` 已安装 |
| `libspa-sys` / `libpipewire-0.3` not found | 安装 `libpipewire-0.3-dev` 和 `libspa-0.2-dev`，然后重新 `npm run tauri:build` |
| `Unable to find libclang`（bindgen） | 安装 `libclang-dev`（或 `clang`），必要时 `export LIBCLANG_PATH=/usr/lib/llvm-*/lib` |
| `unable to find library -lgbm`（链接） | 安装 `libgbm-dev libegl1-mesa-dev libdrm-dev libwayland-dev` |
| `unable to find library -lxdo`（链接） | 安装 `libxdo-dev`（已含在 `scripts/install-linux-build-deps.sh`） |
| 电脑操控异常 | Wayland 限制；试 X11；确认 `libxdo-dev` 已装 |
| 首次编译极慢 | 正常，Rust release 全量编译约 10–30 分钟 |

---

## Web 端开发与部署

Web 端复用同一 Vue 界面，`pointer-core` 由 `server` crate 提供 HTTP/SSE。

### 本地开发

终端 1 — 后端：

```bash
cd pointer-app
npm run server:dev
# 默认 http://127.0.0.1:8787
```

终端 2 — 前端：

```bash
npm run dev:web
# 默认 http://0.0.0.0:1420
```

可选环境变量：

```bash
VITE_WEB_API_BASE=http://127.0.0.1:8787 npm run dev:web
POINTER_SERVER_ADDR=0.0.0.0:8787 npm run server:dev
```

### 生产构建（示意）

**前后端分离**（API 与静态页分开部署）：

```bash
npm run build                              # Vue → dist/
cargo build -p pointer-server --release    # target/release/pointer-server
# 静态页由 Nginx 等托管；构建时可设 VITE_WEB_API_BASE=https://api.example.com
```

**前后端一体化**（推荐：单进程同时提供 API + Web UI）：

```bash
npm run server:build    # 同域前端 + release 二进制
npm run server:start    # 或 ./target/release/pointer-server
```

浏览器打开 `http://127.0.0.1:8787` 即可（API 与页面同端口）。

`dist/` 查找顺序：`POINTER_SERVER_STATIC_DIR` → 当前目录 `dist/` → 可执行文件旁 `dist/` → `target/release/../../dist`。

**配置文件（推荐 Windows / NSSM 部署）**：在 `pointer-server.exe` 同目录放置 `pointer-server.toml` 或 `pointer-server.env`，启动时自动加载；已存在的 OS 环境变量优先。示例见 `server/pointer-server.toml.example`。

```toml
# pointer-server.toml
[server]
addr = "0.0.0.0:8787"
static_dir = "dist"          # 相对路径相对于配置文件所在目录
# 数据目录与日志默认与桌面客户端相同（PointerApp/logs/，debug 为 PointerAppDev），无需配置

[openpointer]
api_base = "https://api.openpointer.cn"
oauth_client_secret = "your-secret"
```

也可使用 dotenv 格式 `pointer-server.env`，或 `POINTER_SERVER_CONFIG=C:\pointer\pointer-server.toml` 指定路径。

仍支持环境变量覆盖（NSSM `AppEnvironmentExtra` 会覆盖文件中的同名项）：

```bash
POINTER_SERVER_ADDR=0.0.0.0:8787 npm run server:start
POINTER_SERVER_STATIC_DIR=/opt/pointer/dist npm run server:start
```

Web 端不提供完整电脑操控；桌面能力（`invoke`、本地存储等）仅在 Tauri 内可用。

---

## 打包通用流程

无论平台，推荐顺序：

```bash
cd pointer-app
npm install
npm run icons          # 首次或更换 icon.png 后
npm run tauri:build      # 或 build:windows / build:macos / build:linux
```

构建时会自动执行 `beforeBuildCommand`（`npm run build`：Vue 类型检查 + Vite 打包到 `dist/`）。

**统一产物根目录：**（在**当前构建系统**上只会出现对应平台的子目录）

```text
src-tauri/target/release/bundle/
├── msi/          # Windows
├── macos/        # macOS（.app）
├── dmg/          # macOS
├── deb/          # Linux
└── appimage/     # Linux（AppImage）
```

| 平台 | 推荐命令 |
|------|----------|
| Windows | `npm run build:windows` |
| macOS | `npm run build:macos`（Universal：`npm run tauri:build -- --target universal-apple-darwin`） |
| Linux | `npm run build:linux` |

**产品名：** Pointer（`src-tauri/tauri.conf.json` → `productName`）

### 相关配置文件

| 文件 | 作用 |
|------|------|
| `src-tauri/tauri.conf.json` | 窗口、bundle 目标、Linux deb/AppImage、Windows NSIS |
| `package.json` | `tauri:dev` / `tauri:build` / `build:*` / `icons` 脚本 |
| `scripts/tauri-build.mjs` | 跨平台 `tauri build`；**仅 Linux** 自动 `NO_STRIP=true`（AppImage） |
| `.github/workflows/release.yml` | 三端 CI 自动打包 |
| `.pointer-build.toml` | 编译期默认配置（非运行时），见 [`docs/internals/pointer-build-toml.md`](../internals/pointer-build-toml.md) |

### 图标

```bash
npm run icons
# 或指定源图：node scripts/generate-app-icons.mjs /path/to/logo.png
```

从源 PNG 生成圆角图标（源图 1254px 时圆角半径 250px，透明四角），并生成各平台尺寸：
- **Windows / Linux**：满幅，无额外留白
- **macOS**（`icon.icns`）：四边各留 10% 透明边距

生成 `32x32.png`、`128x128.png`、`128x128@2x.png`、`icon.ico`、`icon.icns` 等，并同步到 `public/`。

**Windows 桌面图标**在 Rust **链接时**写入 exe（来自 `icons/icon.ico`）。更换 `icon.png` 后需先 `npm run icons`，再重新打包。

若安装包或任务栏仍显示旧图标：

1. 执行完整打包：`npm run build:windows`（不要只跑 `cargo build`）
2. 用**本次**产物安装：`target/release/bundle/msi/Pointer_*_x64_*.msi`（勿用 `bundle/nsis/` 下残留的旧 setup.exe）
3. 先卸载旧版再装新版；开始菜单快捷方式可能缓存图标，可删快捷方式后重装
4. 仍不对时清理后重编：`cargo clean -p pointer-app`，再 `npm run build:windows`

---

## GitHub Actions 三端发版

工作流：`.github/workflows/release.yml`

| Runner | 产物 |
|--------|------|
| `windows-latest` | Windows MSI 安装包 |
| `macos-latest` | Universal macOS（`--target universal-apple-darwin`） |
| `ubuntu-22.04` | Linux deb + AppImage |

**触发方式：**

1. GitHub → Actions → **Release** → Run workflow  
2. 推送版本标签：

```bash
git tag v0.1.0
git push origin v0.1.0
```

CI 步骤：checkout → Node 20 → Rust stable →（Linux 装系统依赖）→ `npm install` → `npm run icons` → `tauri-apps/tauri-action` → 上传 **Draft Release**。

---

## 编译前检查清单

```bash
npm run build          # vue-tsc + vite build
cd src-tauri && cargo check && cd ..
npm run icons
npm run tauri:build
```

### 调试日志

```bash
# Bash / zsh / Linux
RUST_LOG=debug npm run tauri:dev

# PowerShell
$env:RUST_LOG="debug"; npm run tauri:dev
```

---

## 签名与分发（正式发布）

当前配置可生成**未签名**安装包，内测可用。对外发布建议：

| 平台 | 建议 |
|------|------|
| Windows | Authenticode 代码签名 |
| macOS | Developer ID 签名 + Notarization |
| Linux | 按渠道决定是否 GPG 签 deb / 校验 AppImage |

---

## 官网与环境变量（桌面 Release）

Release 安装包已内置 `pointer.readflowai.com` 等生产域名。  
本地 `tauri dev` 默认连本机 `3000/8001/8000`，可用 `POINTER_*` 环境变量覆盖（详见 README「开发」一节）。

---

## 相关文档

| 文档 | 说明 |
|------|------|
| [DEVELOPMENT.md](../../DEVELOPMENT.md) | 日常调试、Skills、常见问题 |
| [README.md](../../README.md) | 项目概览与快速开始 |
| [macos-computer-permissions.md](../internals/macos-computer-permissions.md) | macOS 电脑操控权限 |
| [pointer-build-toml.md](../internals/pointer-build-toml.md) | 编译期默认配置 |

[返回 guides 索引](README.md)
