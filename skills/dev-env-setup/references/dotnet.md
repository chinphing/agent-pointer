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
echo 'export PATH=$PATH:$DOTNET_ROOT' >> ~/.bashrc
source ~/.bashrc
rm /tmp/dotnet.tar.gz
```

## Windows

### 华为镜像下载安装

```powershell
$version = "8.0.405"
$url = "https://mirrors.huaweicloud.com/dotnet/sdk/$version/dotnet-sdk-$version-win-x64.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\dotnet.zip"
Expand-Archive -Path "$env:TEMP\dotnet.zip" -DestinationPath "C:\Program Files\dotnet" -Force
[Environment]::SetEnvironmentVariable("DOTNET_ROOT", "C:\Program Files\dotnet", "Machine")
$path = [Environment]::GetEnvironmentVariable("Path", "Machine")
[Environment]::SetEnvironmentVariable("Path", "$path;C:\Program Files\dotnet", "Machine")
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/dotnet/sdk/8.0.405/`
2. 下载 `dotnet-sdk-8.0.405-win-x64.zip`
3. 解压到 `C:\Program Files\dotnet`
4. 设置系统环境变量 `DOTNET_ROOT` + PATH

## NuGet 华为镜像

创建 `%USERPROFILE%\AppData\Roaming\NuGet\nuget.config`：

```xml
<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <packageSources>
    <clear />
    <add key="nuget-huawei" value="https://mirrors.huaweicloud.com/repository/nuget/v3/index.json" />
  </packageSources>
</configuration>
```

## 验证

```bash
dotnet --version
dotnet new console -o hello && cd hello && dotnet run
```
