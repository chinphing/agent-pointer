# macOS 签名与公证

本地 **Developer ID 签名 + Notarization**，配置保存在 `signing.env`（已 gitignore，勿提交）。

## 快速开始

```bash
cd pointer-app

# 1. 交互式填写 p12 路径、密码、Team ID、Apple ID 等
npm run signing:macos:setup

# 2. 签名 + 公证 + 打包（默认 Universal Binary，Intel + Apple Silicon）
npm run build:macos:signed

# 或：Universal + 仅签名，不公证（内测 / 本机分发）
npm run build:macos:sign-only
```

**签名打包**（`sign-only` / `signed`）默认 Universal，产物：

```text
target/universal-apple-darwin/release/bundle/macos/*.app
target/universal-apple-darwin/release/bundle/dmg/Pointer_*_universal.dmg
```

不签名 `npm run build:macos` 仍为本机 CPU 架构，产物在 `target/release/bundle/`。

## 手动编辑配置

```bash
cp signing/macos/signing.env.example signing/macos/signing.env
# 编辑 signing.env 后：
npm run build:macos:signed
```

### signing.env 字段

| 变量 | 说明 |
|------|------|
| `APPLE_CERTIFICATE_PATH` | `.p12` 路径（相对仓库根或绝对路径） |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 p12 时设置的密码 |
| `APPLE_SIGNING_IDENTITY` | `security find-identity -v -p codesigning` 中的 **Developer ID Application** 名称 |
| `APPLE_TEAM_ID` | 10 位 Team ID（公证必填；仅签名可留空） |
| `APPLE_ID` | Apple 账号邮箱（仅 `build:macos:signed` 需要） |
| `APPLE_PASSWORD` | [App 专用密码](https://support.apple.com/zh-cn/HT204397)（仅公证需要） |
| `MACOS_BUILD_ARCH` | 签名打包默认 `universal`（Intel + Apple Silicon）；`native` = 仅本机 CPU |

也可改用 App Store Connect API Key（见 `signing.env.example` 注释）。

## 其他命令

| 命令 | 说明 |
|------|------|
| `npm run build:macos` | 不签名，本机 CPU 架构 |
| `npm run build:macos:universal` | 不签名，Universal Binary |
| `npm run build:macos:sign-only` | **Universal + 仅签名**，不公证（默认） |
| `npm run build:macos:signed` | **Universal + 签名 + 公证** |
| `npm run build:macos:sign-only -- --icons` | 打包前重新生成图标 |
| `npm run build:macos:signed -- --bundles dmg` | 仅打 DMG |
| `npm run build:macos:signed -- --icons` | 打包前重新生成图标 |
| `npm run build:macos:signed -- --skip-stapling` | 跳过 staple（调试公证） |

## 验证

```bash
APP=target/universal-apple-darwin/release/bundle/macos/Pointer.app
codesign -dv --verbose=4 "$APP"
spctl -a -vv "$APP"
xcrun stapler validate target/universal-apple-darwin/release/bundle/dmg/*.dmg
```

## 证书要求

对外分发需 **Developer ID Application** 证书，不是 **Apple Development**。  
若 `security find-identity` 只有 Development 证书，请在 [Apple Developer](https://developer.apple.com/account/resources/certificates/list) 创建 Developer ID Application 并重新导出 p12。

## CI

GitHub Actions 使用 Repository Secrets（见 `.github/workflows/release.yml`）。  
本地 `signing.env` 与 CI secrets 字段对应，可互相参考。
