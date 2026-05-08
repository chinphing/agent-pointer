# 开发调试说明

本文档用于说明 `Pointer` 的本地开发、调试、检查与常见问题处理流程。

## 环境要求

### 通用依赖

- Node.js 20+
- npm
- Rust stable
- Tauri 2 CLI，由项目依赖提供

检查命令：

```bash
node -v
npm -v
rustc --version
cargo --version
```

### Windows 额外要求

- Microsoft C++ Build Tools
- WebView2 Runtime

### macOS 额外要求

```bash
xcode-select --install
```

### Linux 额外要求

Ubuntu/Debian：

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

## 安装依赖

在项目根目录执行：

```bash
npm install
```

## 启动开发模式

### 桌面端

完整桌面应用调试：

```bash
npm run tauri:dev
```

或：

```bash
npm run tauri dev
```

这会同时启动：

- Vite 前端开发服务
- Tauri 桌面窗口
- Rust 后端服务

### Web 端

Web 端复用同一套 Vue 界面，后端复用 `crates/pointer-core`，由 `server` crate 提供 HTTP/SSE API。

启动 Rust Web 后端：

```bash
npm run server:dev
```

另开一个终端启动 Web 前端：

```bash
npm run dev:web
```

默认 Web API 地址：

```text
http://127.0.0.1:8787
```

如需修改前端访问地址，可设置：

```bash
VITE_WEB_API_BASE=http://127.0.0.1:8787 npm run dev:web
```

Windows PowerShell：

```powershell
$env:VITE_WEB_API_BASE="http://127.0.0.1:8787"; npm run dev:web
```

## 单独调试前端

```bash
npm run dev
```

然后打开终端输出的本地地址。

注意：单独在浏览器中运行时，Tauri 的 `invoke`、事件监听、文件存储等桌面能力不可用，完整功能请使用 `npm run tauri:dev`。

## 打开开发者工具

Tauri 窗口中可使用快捷键打开前端 DevTools：

- Windows/Linux：`Ctrl + Shift + I`
- macOS：`Cmd + Option + I`

可用于查看：

- Console 日志
- Network 请求
- Vue 运行错误
- Tauri `invoke` 调用异常
- 流式事件处理状态

## 模型配置调试

首次启动后，在应用「设置」中填写 DashScope API Key。

默认配置：

```text
Provider: qwen
Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
Model: qwen-plus
```

建议调试流程：

1. 打开设置
2. 填入 DashScope API Key
3. 点击测试连接
4. 发送简单消息，例如 `你好`
5. 测试工具调用，例如 `帮我计算 123 * 456`

## Rust 后端调试

进入 Tauri 后端目录：

```bash
cd src-tauri
```

检查 Rust 编译：

```bash
cargo check
```

运行单元级编译检查时，建议先回到项目根目录执行完整开发命令：

```bash
npm run tauri:dev
```

开启详细日志：

### PowerShell

```powershell
$env:RUST_LOG="debug"; npm run tauri:dev
```

### Bash/zsh

```bash
RUST_LOG=debug npm run tauri:dev
```

## 前端检查

类型检查与前端构建：

```bash
npm run build
```

如果只需要启动 Vite：

```bash
npm run dev
```

## 图标生成

打包前建议生成平台图标：

```bash
npm run icons
```

源图标：

```bash
src-tauri/icons/icon.svg
```

生成后会包含：

- `32x32.png`
- `128x128.png`
- `128x128@2x.png`
- `icon.ico`
- `icon.icns`

## 本地打包检查

当前系统打包：

```bash
npm run tauri:build
```

产物目录：

```bash
src-tauri/target/release/bundle/
```

## 常见问题

### 1. `tauri` 命令找不到

先安装依赖：

```bash
npm install
```

然后使用项目脚本：

```bash
npm run tauri:dev
```

不要依赖全局 `tauri` 命令。

### 2. `npx` 或 `npm` 找不到

说明 Node.js 未安装或未加入 PATH。安装 Node.js 20+ 后重新打开终端。

### 3. Windows 编译失败，提示 C++ 工具链缺失

安装 Microsoft C++ Build Tools，并勾选 C++ 桌面开发相关组件。

### 4. Linux 编译失败，提示 WebKitGTK 缺失

安装 Linux 额外依赖，见本文档「Linux 额外要求」。

### 5. API 请求失败

检查：

- API Key 是否已填写
- Base URL 是否为 `https://dashscope.aliyuncs.com/compatible-mode/v1`
- 模型名是否可用，例如 `qwen-plus`
- 网络是否能访问 DashScope 服务

### 6. 工具调用无响应

检查前端 DevTools Console 和终端日志，重点关注：

- `chat://stream` 事件
- `tool_call_start`
- `tool_call_result`
- `approve_tool_call`

## 推荐开发流程

```bash
npm install
npm run tauri:dev
```

修改代码后：

```bash
npm run build
cd src-tauri
cargo check
```

准备打包前：

```bash
npm run icons
npm run tauri:build
```
