# Python 安装指南

## macOS / Linux

### 安装 pyenv（推荐多版本管理）
```bash
curl -fsSL https://gitee.com/mirrors/pyenv/raw/master/install.sh | bash
echo 'export PYENV_ROOT="$HOME/.pyenv"' >> ~/.zshrc
echo 'command -v pyenv >/dev/null || export PATH="$PYENV_ROOT/bin:$PATH"' >> ~/.zshrc
echo 'eval "$(pyenv init -)"' >> ~/.zshrc
source ~/.zshrc
```

### 安装 Python

```bash
# 使用华为镜像加速编译
export PYTHON_BUILD_MIRROR_URL=https://mirrors.huaweicloud.com/python
pyenv install 3.12.4
pyenv global 3.12.4
python --version
```

### 配置 pip 华为镜像

```bash
pip config set global.index-url https://mirrors.huaweicloud.com/repository/pypi/simple
```

备用镜像：
- 华为: `https://mirrors.huaweicloud.com/repository/pypi/simple`
- 清华: `https://pypi.tuna.tsinghua.edu.cn/simple`

### 验证

```bash
python -c "print('Python works')"
```

## Windows

> **推荐方案：下载华为镜像的 installer，用户级静默安装**（无需管理员权限，避免下载缓慢）

### 1. 查看可用版本

从华为镜像查看最新 Python 版本：

```powershell
Invoke-WebRequest -Uri "https://mirrors.huaweicloud.com/python/" -UseBasicParsing -TimeoutSec 15 -OutFile "$env:TEMP\py_ver.html"
Get-Content "$env:TEMP\py_ver.html" | Select-String '(\d+\.\d+\.\d+)/' | Select-Object -Last 10
```

选择最新的稳定版（如 3.14.5）。

### 2. 下载安装包（华为镜像）

```powershell
# 替换为实际版本号
$version = "3.14.5"
$url = "https://mirrors.huaweicloud.com/python/$version/python-$version-amd64.exe"
Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 300 -OutFile "$env:TEMP\python-$version-amd64.exe"
```

### 3. 用户级静默安装（无需管理员）

```powershell
Start-Process -FilePath "$env:TEMP\python-$version-amd64.exe" -ArgumentList '/quiet InstallAllUsers=0' -Wait
```

> 参数说明：`InstallAllUsers=0` 安装到 `%LOCALAPPDATA%\Programs\Python\PythonXXX\`，不写入系统注册表，无需管理员权限。

### 4. 检查 PATH

安装器不会自动添加用户级安装到 PATH，需手动确认：

```powershell
python --version
```

如果提示找不到，手动添加 PATH：

```powershell
$pythonPath = "$env:LOCALAPPDATA\Programs\Python\Python314\"
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$newPath = @($userPath -split ';' | Where-Object {
    $_ -notlike "*Python*" -and $_ -ne ''
}) -join ';'
$newPath = "$pythonPath;$newPath"
[Environment]::SetEnvironmentVariable("Path", $newPath, "User")
```

### 5. 配置 pip 华为镜像

```powershell
pip config set global.index-url https://mirrors.huaweicloud.com/repository/pypi/simple
```

### 6. 验证

```powershell
python --version
pip --version
python -c "print('Python 环境就绪')"
```

---

## 国内镜像说明

| 资源 | 镜像地址 |
|------|---------|
| Python 安装包 | `https://mirrors.huaweicloud.com/python/` |
| pip 镜像 | `https://mirrors.huaweicloud.com/repository/pypi/simple` |

## 踩坑备忘录

| 问题 | 原因 | 解决方案 |
|------|------|---------|
| `InstallAllUsers=1` 安装失败 | 当前用户无管理员权限 | 改用 `InstallAllUsers=0` 用户级安装 |
| 安装后 `python` 找不到 | 用户级安装不自动添加 PATH | 手动 `[Environment]::SetEnvironmentVariable` 添加 |
| `winget install python` 极慢/失败 | 国内网络访问微软 CDN | 改用华为镜像直接下载 exe |
| 下载安装包失败或超时 | 网络波动 | 增加 `-TimeoutSec 300` 参数重试 |
