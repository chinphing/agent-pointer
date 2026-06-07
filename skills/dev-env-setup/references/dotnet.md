# .NET SDK (C#) 安装指南

## macOS

### 华为镜像下载安装

```bash
# 下载 .NET SDK 8.0（当前 LTS）
version="8.0.405"
arch="arm64"  # Apple Silicon
# arch="x64"  # Intel
wget https://mirrors.huaweicloud.com/dotnet/sdk/$version/dotnet-sdk-$version-osx-$arch.tar.gz -O /tmp/dotnet.tar.gz

# 解压安装
sudo mkdir -p /usr/share/dotnet
sudo tar -C /usr/share/dotnet -xzf /tmp/dotnet.tar.gz
sudo ln -sf /usr/share/dotnet/dotnet /usr/local/bin/dotnet
rm /tmp/dotnet.tar.gz
```

## Linux

```bash
version="8.0.405"
wget https://mirrors.huaweicloud.com/dotnet/sdk/$version/dotnet-sdk-$version-linux-x64.tar.gz -O /tmp/dotnet.tar.gz
sudo mkdir -p /usr/share/dotnet
sudo tar -C /usr/share/dotnet -xzf /tmp/dotnet.tar.gz
sudo ln -sf /usr/share/dotnet/dotnet /usr/local/bin/dotnet
echo 'export DOTNET_ROOT=/usr/share/dotnet' >> ~/.bashrc
echo 'export PATH=$PATH:/usr/share/dotnet' >> ~/.bashrc
source ~/.bashrc
```

## Windows

### 华为镜像下载安装

```powershell
$version = "8.0.405"
$url = "https://mirrors.huaweicloud.com/dotnet/sdk/$version/dotnet-sdk-$version-win-x64.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\dotnet.zip"
Expand-Archive -Path "$env:TEMP\dotnet.zip" -DestinationPath "C:\Program Files\dotnet" -Force
[Environment]::SetEnvironmentVariable("PATH", "C:\Program Files\dotnet;$env:PATH", "Machine")
[Environment]::SetEnvironmentVariable("DOTNET_ROOT", "C:\Program Files\dotnet", "Machine")
```

---

## NuGet 华为镜像

```bash
dotnet nuget add source https://mirrors.huaweicloud.com/repository/nuget/ -n huawei
```

### 验证

```bash
dotnet --version
dotnet --list-sdks
```
