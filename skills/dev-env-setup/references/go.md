# Go 安装指南

## macOS

### 华为镜像下载安装

```bash
# 确认架构
arch_name="arm64"   # Apple Silicon (M1/M2/M3)
# arch_name="amd64"  # Intel
version="1.23.0"
wget https://mirrors.huaweicloud.com/go/go${version}/go${version}.darwin-${arch_name}.tar.gz -O /tmp/go.tar.gz
sudo tar -C /usr/local -xzf /tmp/go.tar.gz
echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.zshrc
source ~/.zshrc
rm /tmp/go.tar.gz
```

## Linux

### 华为镜像下载安装

```bash
version="1.23.0"
wget https://mirrors.huaweicloud.com/go/go${version}/go${version}.linux-amd64.tar.gz -O /tmp/go.tar.gz
sudo tar -C /usr/local -xzf /tmp/go.tar.gz
echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.bashrc
source ~/.bashrc
rm /tmp/go.tar.gz
```

## Windows

### 前置要求

- Windows 10 / 11（64 位）
- PowerShell（默认已带）

### 华为镜像下载安装

**注意：** 华为镜像上 Go 的目录结构为 `go/${version}/` 子目录，URL 中需要包含版本目录。

```powershell
# 1. 设置版本（查询最新版：访问 https://mirrors.huaweicloud.com/go/ 查看 goX.Y.Z/ 目录）
$version = "1.21.13"

# 2. 下载（华为镜像，国内速度快）
$url = "https://mirrors.huaweicloud.com/go/go$version/go$version.windows-amd64.zip"
$out = "$env:TEMP\go.zip"
Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing

# 3. 解压（用 tar 更快，Windows 10/11 已内置）
tar -xf $out -C "$env:TEMP"

# 4. 移动到目标目录
if (Test-Path "C:\Program Files\Go") { Remove-Item "C:\Program Files\Go" -Recurse -Force }
Move-Item "$env:TEMP\go" "C:\Program Files\Go" -Force

# 5. 设置系统环境变量（Machine 级别 PATH）
[Environment]::SetEnvironmentVariable(
    "PATH",
    "C:\Program Files\Go\bin;$([Environment]::GetEnvironmentVariable('PATH', 'Machine'))",
    "Machine"
)

# 6. 设置 GOPATH 和用户 PATH
$goPath = "$env:USERPROFILE\go"
[Environment]::SetEnvironmentVariable("GOPATH", $goPath, "User")
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$goPath\bin*") {
    [Environment]::SetEnvironmentVariable("PATH", "$goPath\bin;$userPath", "User")
}

# 7. 清理临时文件
Remove-Item $out -Force

Write-Host "安装完成！请重启终端或执行:"
Write-Host "`$env:PATH = [Environment]::GetEnvironmentVariable('PATH', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('PATH', 'User')"
```

### 验证安装

```powershell
# 在新终端中执行，或使用完整路径
& "C:\Program Files\Go\bin\go.exe" version
# 期望输出: go version go1.21.13 windows/amd64
```

---

## GOPROXY 华为镜像（所有平台通用

配置 Go modules 代理，加速依赖下载：

```bash
go env -w GO111MODULE=on
go env -w GOPROXY=https://mirrors.huaweicloud.com/proxy/golang,direct
```

### 完整验证

```bash
go version
go env GOPROXY
go env GOROOT
go env GOPATH
```

### 快速测试

创建 `hello.go`：

```go
package main

import "fmt"

func main() {
    fmt.Println("Hello, Go!")
}
```

运行：

```bash
go run hello.go
# 输出: Hello, Go!
```

---

## 注意事项

1. **华为镜像版本目录结构**：URL 为 `https://mirrors.huaweicloud.com/go/go${version}/go${version}.{os}-{arch}.{ext}`，中间有版本子目录 `/go${version}/`
2. **版本查询**：访问 https://mirrors.huaweicloud.com/go/ 查看可用的 `goX.Y.Z/` 目录
3. **Windows zip 结构**：zip 内文件在 `go/` 子目录下（不是直接根目录文件），解压后需要将整个 `go/` 目录移动到目标位置
4. **GOPATH**：默认为 `%USERPROFILE%\go`，其下的 `bin` 目录已加入用户 PATH
5. **镜像未及时更新时**：可改用官方源 `https://go.dev/dl/`，再配合 GOPROXY 加速下载
