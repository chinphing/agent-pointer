# Windows：Chocolatey 安装与使用

Chocolatey 是 dev-env-setup 在 Windows 上**所有软件安装的首选方式**。
安装任意软件时，**先查 Chocolatey 是否有包**，再考虑其它途径。

各语言/工具专项指南未特别声明时，同样优先 `choco install <包名> -y`。

## 安装 Chocolatey

需**管理员 PowerShell**。若 `choco -v` 报错或找不到命令，先执行：

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
```

完成后**关闭并重新打开**管理员 PowerShell，验证：

```powershell
choco -v
```

## 安装软件

```powershell
choco search <keyword>
choco install <package> -y
```

安装后**新开终端**，必要时将 `C:\ProgramData\chocolatey\bin` 加入 PATH。

## 查找包名

```powershell
choco search <keyword>
```

或在 https://community.chocolatey.org/packages 搜索。

常见包名速查见 **references/general-software.md**。

## Checksum 校验失败

上游软件更新了安装包，Chocolatey 包内记录的 checksum 尚未同步时，安装/升级会报错，例如：

```text
Checksum for 'xxx-setup.exe' did not meet the expected checksum
ERROR: Checksum failed
```

**原因：** 官方安装包已换版，社区包维护者还未更新 checksum，属常见现象，不代表本机环境损坏。

### 处理顺序（由稳妥到备选）

**1. 刷新 Chocolatey 后重试**

```powershell
choco upgrade chocolatey -y
choco install <package> -y
```

**2. 清缓存后重试**

```powershell
choco uninstall <package> -y
Remove-Item -Recurse -Force "$env:TEMP\chocolatey" -ErrorAction SilentlyContinue
choco install <package> -y
```

**3. 忽略 checksum 继续安装（常用）**

确认该包来自 Chocolatey 社区仓库、下载地址为官方源时，可跳过校验：

```powershell
choco install <package> -y --ignore-checksums
```

升级同理：

```powershell
choco upgrade <package> -y --ignore-checksums
```

批量升级时仅对单个失败包忽略：

```powershell
choco upgrade all -y --except="<其它包>"
choco upgrade <package> -y --ignore-checksums
```

**4. 安装指定旧版本**

包支持多版本时，可先装维护者已校验通过的版本：

```powershell
choco search <package> --all-versions
choco install <package> --version=<版本号> -y
```

**5. 仍失败则换途径**

按 **references/general-software.md** 例外流程改用 winget 或官方安装包。

### 使用 `--ignore-checksums` 时注意

- 会跳过安装包完整性校验；仅在对该包来源可信时使用（社区包 + 官方下载 URL）。
- 企业环境若安全策略禁止，改用 winget / 官方安装包，并向用户说明原因。
- 可告知用户：此为 Chocolatey 社区包滞后于上游版本的已知问题，安装成功后功能通常不受影响。

## 注意事项

- 需管理员权限。
- 企业环境若策略禁止 Chocolatey，改用 winget 或官方安装包（见 **general-software.md**）。
- 某 references 文件若写明「不用 Chocolatey」或指定唯一安装方式，以该文件为准。
- Chocolatey 无对应包时，再尝试 `winget search` / `winget install`。
- Checksum 失败见上文专节；优先 `--ignore-checksums`，勿轻易放弃 choco。
