# Go 安装指南

## macOS

### 华为镜像下载安装

```bash
# 确认架构
arch_name="arm64"   # Apple Silicon (M1/M2/M3)
# arch_name="amd64"  # Intel
version="1.23.0"
wget https://mirrors.huaweicloud.com/go/go${version}.darwin-${arch_name}.tar.gz -O /tmp/go.tar.gz
sudo tar -C /usr/local -xzf /tmp/go.tar.gz
echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.zshrc
source ~/.zshrc
rm /tmp/go.tar.gz
```

## Linux

```bash
version="1.23.0"
wget https://mirrors.huaweicloud.com/go/go${version}.linux-amd64.tar.gz -O /tmp/go.tar.gz
sudo tar -C /usr/local -xzf /tmp/go.tar.gz
echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.bashrc
source ~/.bashrc
rm /tmp/go.tar.gz
```

## Windows

### 华为镜像下载安装

```powershell
$version = "1.23.0"
$url = "https://mirrors.huaweicloud.com/go/go$version.windows-amd64.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\go.zip"
Expand-Archive -Path "$env:TEMP\go.zip" -DestinationPath "C:\" -Force
# 添加 PATH
[Environment]::SetEnvironmentVariable("Path", "$env:Path;C:\go\bin", "User")
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/go/`
2. 下载 `go1.23.0.windows-amd64.zip`
3. 解压到 `C:\go`
4. 系统环境变量 PATH 添加 `C:\go\bin`

## ⚡ Go Proxy（华为镜像）

```bash
go env -w GOPROXY=https://mirrors.huaweicloud.com/repository/go,direct
```

备用：`https://goproxy.cn`（七牛云）

### 验证

```bash
go version
go env GOPROXY
```
