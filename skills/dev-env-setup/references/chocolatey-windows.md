# Windows：Chocolatey 安装与使用

Chocolatey 是 dev-env-setup 在 Windows 上的**通用软件安装途径**。
各语言/工具指南未特别声明时，均可尝试 `choco install <包名> -y`。

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
choco install <package> -y
```

安装后**新开终端**，必要时将 `C:\ProgramData\chocolatey\bin` 加入 PATH。

## 查找包名

```powershell
choco search <keyword>
```

或在 https://community.chocolatey.org/packages 搜索。

## 注意事项

- 需管理员权限；企业环境若策略禁止，改用该工具指南中的其它途径（winget、镜像直链等）。
- 某 references 文件若写明「不用 Chocolatey」或指定唯一安装方式，以该文件为准。
