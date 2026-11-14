# Windows 7 客户端部署

Pointer 桌面客户端可以运行在 Windows 7 SP1 x64 上，但需要额外的系统组件和**专门构建的产物**。普通 Windows 10/11 安装包在 Win7 上装得上、跑不起来。

本文覆盖：系统依赖安装 → 客户端安装 → 专门产物的构建方式 → 排错。

---

## 1. 适用范围

| 项 | 要求 |
|---|---|
| 系统 | Windows 7 **SP1** x64 |
| 架构 | 仅 64 位（不支持 32 位 Win7） |
| 磁盘 | 约 500 MB（含两个运行时） |
| 前置组件 | UCRT 运行库、WebView2 109 |

> Windows 8 / 8.1 未验证。Windows 10 及以上直接用普通安装包，无需本文步骤。

---

## 2. 为什么需要特殊处理

三件事叠加，导致普通安装包在 Win7 上不可用：

1. **Rust 从 1.78 起不再支持 Win7**。`x86_64-pc-windows-msvc` 目标的标准库改用 Windows 8+ 的 futex 同步原语，产出的二进制在 Win7 上启动即报缺 DLL。必须用 `x86_64-win7-windows-msvc` 目标重新构建。
2. **WebView2 官方在线安装器在 Win7 上跑不起来**。微软已停止 Win7 支持，最后一个可用版本是 **109**。
3. **Win7 不自带 UCRT**（通用 C 运行库），现代 Rust 产物依赖它。

---

## 3. 系统组件安装

### 3.1 安装 UCRT / VC++ 运行库

**先装这个**，否则客户端启动会报缺 DLL。

下载（官方，支持 Win7）：

```
https://aka.ms/vs/17/release/vc_redist.x64.exe
```

约 25 MB。安装过程中会看到 `Windows7_MSU_x64` 这一项——那就是 UCRT 补丁，**不要中断**，中断会导致运行库处于不完整状态，后续启动报 `0xc0000005`。

图形界面安装：勾选许可协议 → Install。全程在模拟环境中约 5–8 分钟。

### 3.2 安装 WebView2 运行时（109）

**不要用官方 Evergreen 引导程序**（`MicrosoftEdgeWebView2Setup.exe`）。它在 Win7 上会失败：

```
Error 1723: Loading WebView2 failed - HTTP/1.1 403 Forbidden
Installation Aborted
```

原因是 Win7 的 TLS 栈过旧，且新版安装器调用了 Win7 没有的 `GetPackagesByPackageFamily`。

**必须用 109 的独立离线安装包**：

```
MicrosoftEdgeWebView2RuntimeInstallerX64.exe   145,772,880 字节
```

这个文件微软已从官网下架，可从 archive.org 的 WebView2 存档取到。装完窗口显示 `Installation complete.` 即成功。

验证是否装好：注册表 `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` 下的 `pv` 值应为 `109.0.1518.78`。

### 3.3 安装 Pointer 客户端

运行 Win7 专用产物（见第 4 节）：

- 安装包：`Pointer-*-x64-setup.exe`（NSIS）
- 或直接运行 `pointer-app.exe`（免安装）

安装目录：`C:\Users\<用户>\AppData\Local\Pointer`

---

## 4. 构建 Win7 产物

普通 release 产物**不能**在 Win7 上运行。需要用 `x86_64-win7-windows-msvc` 目标重新编译。

### 4.1 CI

工作流：`.github/workflows/win7-client.yml`，只构建 Windows，约 20 分钟。

产物：`pointer-app.exe`（约 62 MB，含内嵌前端）+ NSIS 安装包。

### 4.2 构建要点

以下每一项都对应一个实际踩过的坑：

| 配置 | 原因 |
|---|---|
| nightly 工具链 + `rust-src` | `-Z build-std` 需要从源码编译标准库 |
| `RUSTUP_TOOLCHAIN=nightly` | `rust-toolchain.toml` 里写死了 `channel = "stable"`，会覆盖 nightly |
| `ilammy/msvc-dev-cmd` | `cl.exe` 默认不在 PATH，`aws-lc-sys` 等 C 依赖需要它 |
| 屏蔽 Git 的 `link.exe` | Git for Windows 自带 POSIX 版 `link.exe`，会抢占 MSVC 链接器，报 "returned an unexpected error" |
| `--cfg target_vendor="pc"` + `-A explicit_builtin_cfgs_in_flags` | 目标三元组里 vendor 是 `win7`，`ctor` 等 crate 只认 `pc`，不加会报 "not supported on the current target" |
| 手工补 `-L native=<windows_*_msvc/lib>` | `windows_x86_64_msvc` 的构建脚本把三元组写死，不为新目标输出库搜索路径，否则报 `LNK1181` |
| `--features tauri/custom-protocol` | 不开这个特性，应用会去连 `http://127.0.0.1:1420` 开发服务器，界面显示"连接被拒" |
| 构建前剥离 `xcap` 的 `wgc` 特性 | WGC 后端依赖 WinRT（Win10+）。剥离后 xcap 自动回退到 GDI 后端 |
| 构建后运行 `scripts/patch-win7-imports.py` | 见下节 |

### 4.3 Windows 8 导入项补丁

即使标准库已经是 Win7 兼容的，第三方 crate 仍会静态导入少量 Windows 8+ 函数。Windows 加载器在进程启动时解析**所有**静态导入，缺一个就报：

```
pointer-app.exe - Entry Point Not Found
The procedure entry point EventSetInformation could not be located in advapi32.dll
```

目前已知的是 `EventSetInformation`（ETW，Windows 8 新增）。代码里没有任何地方调用它——它是被 MSVC 导入库的分组机制带进来的：只要 `EventRegister` 这类符号被需要，整个 `advapi32.dll` 成员都会被链接。

**为什么不在源码里定义这个符号**：试过，`LNK2005` 重复定义。`/FORCE:MULTIPLE` 虽然能链接通过，但导入表里那一项仍然存在，Win7 上照样起不来。

所以采用构建后改写导入名：`scripts/patch-win7-imports.py` 把导入表里的名字就地改成同 DLL 中 Win7 存在的函数。脚本在改写后会**再检查一遍**，名字若仍存在就让构建失败，避免工具链变化后悄悄产出跑不起来的包。

### 4.4 本地构建

```bash
rustup toolchain install nightly --component rust-src
export RUSTUP_TOOLCHAIN=nightly
# 按 4.2 配置 .cargo/config.toml 与 xcap 特性
cargo build --release -p pointer-app \
  --features tauri/custom-protocol \
  -Z build-std=std,panic_abort \
  --target x86_64-win7-windows-msvc
python3 scripts/patch-win7-imports.py target/x86_64-win7-windows-msvc/release/pointer-app.exe
```

必须在 Windows 构建机上执行（MSVC 工具链），或在 Linux/macOS 上按官方文档配置 `clang-cl` + `lld-link` + `xwin` 交叉编译。

---

## 5. 已知限制

### 5.1 截图后端降级为 GDI

Win7 产物使用 GDI 截图（`BitBlt`），而不是 Windows 10+ 的 WGC：

| 项 | Win10/11（WGC） | Win7（GDI） |
|---|---|---|
| 截图速度 | 快（GPU） | 慢，1080p 约 30–60 ms |
| 硬件加速窗口 | 能截 | **可能全黑**（DirectX 游戏、部分播放器） |
| 被遮挡窗口 | 能截 | 截到的是遮挡物 |
| 多显示器 | 支持 | 支持 |

对"截桌面给视觉模型看"这类用途影响有限；如果用户要在全屏游戏里用 computer-use，GDI 不满足。

### 5.2 其他

- **不支持 32 位 Win7**。
- **透明窗口依赖 DWM 合成**。Win7 若运行在基本主题或非 WDDM 显卡驱动下，窗口可能完全透明（进程正常、界面不可见）。
- **WebView2 停留在 109**，Chromium 内核落后当前版本约两年。前端若用到较新 Web API 会不可用。

---

## 6. 排错对照

| 现象 | 原因 | 处理 |
|---|---|---|
| `api-ms-win-core-synch-l1-2-0.dll` 缺失 | UCRT 没装，或产物是用普通目标构建的 | 装 3.1；确认产物来自 `win7-client.yml` |
| `0xc000007b` | 运行库装了一半（安装被中断） | 重跑 `vc_redist.x64.exe` 让它装完 |
| `0xc0000005` | 访问冲突，通常也是运行库不完整 | 同上；必要时 `sfc /scannow` |
| `api-ms-win-core-winrt-l1-1-0.dll` 缺失 | 产物启用了 WGC 截图后端 | 构建时剥离 `xcap` 的 `wgc` 特性 |
| `EventSetInformation` 找不到入口点 | 产物没跑补丁脚本 | 见 4.3 |
| 界面显示 `127.0.0.1 refused to connect` | 产物没开 `custom-protocol` | 构建加 `--features tauri/custom-protocol` |
| WebView2 安装报 `Error 1723 / 403 Forbidden` | 用了在线引导程序 | 改用 109 独立离线包 |
| 进程在跑但看不到窗口 | 透明窗口 + 无 DWM 合成 | 改用 Aero 主题，或安装支持 WDDM 的显卡驱动 |

---

## 7. 验证清单

装完后按顺序确认：

1. `C:\Windows\System32\ucrtbase.dll` 存在
2. 注册表 WebView2 版本为 `109.0.1518.78`
3. 双击 `pointer-app.exe`，无错误弹窗
4. 主窗口出现，侧栏可见 `New task` / `Skills` / `Connections`
5. 任务管理器中除 `pointer-app.exe` 外应有多个 `msedgewebview2.exe` 进程——没有则说明 WebView2 未初始化
