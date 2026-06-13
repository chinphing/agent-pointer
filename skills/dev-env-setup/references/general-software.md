# 通用软件安装（非开发语言专项）

本指南适用于**任意桌面/命令行软件**，不在 `references/` 各语言专项文件覆盖范围内时使用。

## Windows：Chocolatey 优先（强制）

在 Windows 上安装**任何软件**，默认流程：

1. 确认 Chocolatey 可用（`choco -v`）；不可用则先读 **references/chocolatey-windows.md** 安装。
2. 搜索包名：`choco search <关键词>` 或浏览 https://community.chocolatey.org/packages
3. 安装：`choco install <package> -y`（管理员 PowerShell）
4. 若报 **Checksum failed**（上游更新导致校验不匹配），按 **references/chocolatey-windows.md**「Checksum 校验失败」处理；常用：`choco install <package> -y --ignore-checksums`
5. **新开终端**，验证命令或应用是否可用。

**不要**因 checksum 失败就直接放弃 Chocolatey 改官网下载；先按上述步骤处理，仍不行再走下方「例外」。

### 常见软件 Chocolatey 包名示例

| 软件 | 包名 |
|------|------|
| VS Code | `vscode` |
| Chrome | `googlechrome` |
| Firefox | `firefox` |
| 7-Zip | `7zip` |
| Notepad++ | `notepadplusplus` |
| Docker Desktop | `docker-desktop` |
| Postman | `postman` |
| Redis | `redis-64` |
| PostgreSQL | `postgresql` |
| MySQL | `mysql` |
| WSL | `wsl2` |
| Git | `git` |
| Node.js（快速装） | `nodejs-lts` |
| Python（快速装） | `python` |

包名以 `choco search` 结果为准；上表仅作常见参考。

### Windows 例外（才不用 choco）

- Chocolatey 仓库**无对应包**，且 winget / 官方安装包是唯一来源
- 企业策略**明确禁止** Chocolatey
- 对应 `references/` 专项指南写明**禁用 Chocolatey** 或指定唯一安装方式（以专项为准）
- 用户明确要求手动安装

例外时优先顺序：**winget** → 官方镜像/直链 → 其它。

```powershell
winget search <关键词>
winget install <包ID> --accept-package-agreements --accept-source-agreements
```

## macOS：Homebrew 优先

1. 确认 Homebrew 可用（`brew -v`）；未安装：

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

国内可设镜像（如清华）后再安装，或安装后配置 `HOMEBREW_API_DOMAIN` / `HOMEBREW_BOTTLE_DOMAIN`。

2. 搜索：`brew search <关键词>`
3. 安装：`brew install <formula>` 或 GUI 应用 `brew install --cask <cask>`
4. 验证：`which <命令>` 或从启动台打开应用

## Linux：发行版包管理器优先

| 发行版 | 搜索 | 安装 |
|--------|------|------|
| Debian/Ubuntu | `apt search <关键词>` | `sudo apt install <包名>` |
| Fedora/RHEL | `dnf search <关键词>` | `sudo dnf install <包名>` |
| Arch | `pacman -Ss <关键词>` | `sudo pacman -S <包名>` |

国内可换源（清华、中科大、阿里云等）后再安装。

## 安装后验证

无论平台，安装完成后应：

1. **新开终端**（或重启 shell）使 PATH 生效
2. 运行 `--version` / `-v` 或启动应用确认成功
3. 失败时检查 PATH、权限、是否需要重启

## 与开发语言专项的关系

- 用户要装 **Node/Python/Java** 等开发环境 → 优先读对应专项 `references/*.md`（版本管理、镜像源更完整）
- 用户要装 **VS Code、Chrome、数据库客户端** 等通用软件 → 读本文件
- Windows 上即使走专项指南，**仍优先尝试 Chocolatey**（专项内「首选 choco」与本文一致）
