# macOS Computer 权限引导（桌面端）

## 范围

仅 **Tauri macOS 桌面客户端**，在用户首次通过 **Computer** 智能体发送消息前拦截。

## 权限项

| 权限 | 检测 API | 用户操作 |
|------|----------|----------|
| 辅助功能（第 1 步，鼠标/键盘） | `AXIsProcessTrusted` | 打开「辅助功能」设置页 + **原生浮动面板**：拖拽 Pointer 图标到列表 |
| 屏幕录制（第 2 步） | `CGPreflightScreenCaptureAccess` | 打开「屏幕录制」设置页 + **原生浮动面板**：拖拽 Pointer 图标到列表 |

辅助功能无法完全在应用内勾选，拖拽 `.app` 到系统设置列表是 macOS 标准授权方式。

检测逻辑：仅 **`CGPreflightScreenCaptureAccess`**。进入向导**第 2 步（屏幕录制）**时调用一次 **`CGRequestScreenCaptureAccess`**，将当前进程登记进「屏幕录制」列表（不替代 preflight 检测）。设置里已启用时 preflight 仍可能返回 false（macOS 已知现象）。

**不要用 xcap 截图成功判断授权**：无 TCC 时仍可 `capture_image()` 得到非空图，但通常只有壁纸/空白桌面、不含其他 App 窗口，会造成假阳性。

若设置里已启用但长时间不进入下一步：请从 **应用程序文件夹** 里的 Pointer.app 启动（不要从 DMG 卷内直接打开），或完全退出后重新打开；向导提供「先进入下一步」/「仍要继续」兜底。辅助功能偶发需重启后 `AXIsProcessTrusted` 才变 true。

## 实现位置

- Rust：`src-tauri/src/macos_computer_permissions.rs`（检测、请求、拖拽引导 `NSPanel`）
- Tauri commands：`begin_macos_permission_drag_flow` / `dismiss_macos_permission_drag_guide`
- 前端向导：`src/components/chat/MacosComputerPermissionsModal.vue`
- 接入：`Composer.vue` 在 `listComputerMonitors` 之前检查权限
- `src-tauri/Info.plist`：`NSScreenCaptureUsageDescription`、`NSAppleEventsUsageDescription`

## 开发模式

`cargo tauri dev` 下可执行文件可能不在 `.app` 包内，拖拽授权需使用 **打包后的 .app**，或在系统设置中手动添加。向导会显示相应提示（`runningFromAppBundle: false`）。

## 测试重置（本机）

```bash
tccutil reset ScreenCapture com.pointer.desktop
tccutil reset Accessibility com.pointer.desktop
```

（Bundle ID 以 `tauri.conf.json` 中 `identifier` 为准。）
