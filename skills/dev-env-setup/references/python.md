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
python -c "print('Python OK')"
pip list
```

## Windows

### 下载安装包

```powershell
# 从华为镜像下载
$version = "3.12.4"
$url = "https://mirrors.huaweicloud.com/python/$version/python-$version-amd64.exe"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\python-installer.exe"
# 静默安装（自动加入 PATH）
Start-Process -FilePath "$env:TEMP\python-installer.exe" -ArgumentList "/quiet InstallAllUsers=1 PrependPath=1" -Wait
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/python/3.12.4/`
2. 下载 `python-3.12.4-amd64.exe`
3. 安装时勾选 **"Add Python to PATH"**

### 配置 pip 华为镜像

```powershell
pip config set global.index-url https://mirrors.huaweicloud.com/repository/pypi/simple
```

### 验证

```powershell
python --version
pip --version
```
