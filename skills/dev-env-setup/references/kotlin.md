# Kotlin 安装指南

## macOS / Linux — SDKMAN（推荐）

```bash
curl -s "https://gitee.com/sdkman/sdkman/raw/master/bin/install" | bash
source "$HOME/.sdkman/bin/sdkman-init.sh"
sdk install kotlin
kotlin -version
```

## Windows

### 方案 A：SDKMAN + WSL 或 Git Bash（推荐）

在 Git Bash 中执行上面的 macOS/Linux 命令即可。

### 方案 B：手动下载（原生 cmd/PowerShell）

```powershell
# 从华为镜像下载 Kotlin 编译器
$version = "2.1.0"  # 前往 https://mirrors.huaweicloud.com/kotlin/ 查看最新版
$url = "https://mirrors.huaweicloud.com/kotlin/kotlin-compiler-$version.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\kotlin.zip"
Expand-Archive -Path "$env:TEMP\kotlin.zip" -DestinationPath "C:\kotlin" -Force
[Environment]::SetEnvironmentVariable("PATH", "$env:PATH;C:\kotlin\kotlinc\bin", "User")
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/kotlin/`
2. 下载最新版 `kotlin-compiler-*.zip`
3. 解压到 `C:\kotlin`
4. 将 `C:\kotlin\kotlinc\bin` 加入 PATH

### 验证

```bash
kotlin -version
kotlinc -version
```
