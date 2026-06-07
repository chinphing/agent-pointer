# Java 安装指南（JDK 21 LTS）

## macOS

### 清华源下载安装

```bash
# 下载 JDK 21 清华镜像（arm64 / x64 按需选择）
arch_name="aarch64"  # Apple Silicon
# arch_name="x64"    # Intel
wget https://mirrors.tuna.tsinghua.edu.cn/Adoptium/21/jdk/${arch_name}/mac/OpenJDK21U-jdk_${arch_name}_mac_hotspot_21.0.11_10.tar.gz -O /tmp/jdk21.tar.gz

# 解压安装
sudo tar -C /Library/Java/JavaVirtualMachines -xzf /tmp/jdk21.tar.gz
rm /tmp/jdk21.tar.gz

# 设置 JAVA_HOME
echo 'export JAVA_HOME=$(/usr/libexec/java_home -v 21)' >> ~/.zshrc
echo 'export PATH=$JAVA_HOME/bin:$PATH' >> ~/.zshrc
source ~/.zshrc
```

## Linux

### 清华源下载安装

```bash
wget https://mirrors.tuna.tsinghua.edu.cn/Adoptium/21/jdk/x64/linux/OpenJDK21U-jdk_x64_linux_hotspot_21.0.11_10.tar.gz -O /tmp/jdk21.tar.gz
sudo tar -C /usr/local -xzf /tmp/jdk21.tar.gz
sudo mv /usr/local/jdk-21.0.11+10 /usr/local/jdk-21
echo 'export JAVA_HOME=/usr/local/jdk-21' >> ~/.bashrc
echo 'export PATH=$JAVA_HOME/bin:$PATH' >> ~/.bashrc
source ~/.bashrc
```

## Windows

### 方案一：BITS 下载（推荐，断点续传稳定）

```powershell
# 下载 JDK 21（清华源）
$url = "https://mirrors.tuna.tsinghua.edu.cn/Adoptium/21/jdk/x64/windows/OpenJDK21U-jdk_x64_windows_hotspot_21.0.11_10.zip"
$zipPath = "$env:TEMP\jdk21.zip"

Start-BitsTransfer -Source $url -Destination $zipPath -Priority High

# 解压安装
Expand-Archive -Path $zipPath -DestinationPath "C:\Program Files\Java" -Force
Remove-Item $zipPath -Force

# 配置用户级环境变量（无需管理员权限）
$javaHome = "C:\Program Files\Java\jdk-21.0.11+10"
[Environment]::SetEnvironmentVariable("JAVA_HOME", $javaHome, "User")
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$javaHome\bin*") {
    [Environment]::SetEnvironmentVariable("PATH", "$javaHome\bin;$userPath", "User")
}
```

### 方案二：Invoke-WebRequest

```powershell
$url = "https://mirrors.tuna.tsinghua.edu.cn/Adoptium/21/jdk/x64/windows/OpenJDK21U-jdk_x64_windows_hotspot_21.0.11_10.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\jdk21.zip"
Expand-Archive -Path "$env:TEMP\jdk21.zip" -DestinationPath "C:\Program Files\Java" -Force

# 配置环境变量（同上）
$javaHome = "C:\Program Files\Java\jdk-21.0.11+10"
[Environment]::SetEnvironmentVariable("JAVA_HOME", $javaHome, "User")
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$javaHome\bin*") {
    [Environment]::SetEnvironmentVariable("PATH", "$javaHome\bin;$userPath", "User")
}
```

### 方案三：手动安装

1. 打开 https://mirrors.tuna.tsinghua.edu.cn/Adoptium/21/jdk/x64/windows/
2. 下载 `OpenJDK21U-jdk_x64_windows_hotspot_21.0.11_10.zip`
3. 解压到 `C:\Program Files\Java\`
4. 设置用户环境变量 `JAVA_HOME` = `C:\Program Files\Java\jdk-21.0.11+10`
5. 在 `PATH` 中添加 `%JAVA_HOME%\bin`

---

## Maven 安装

### Windows

```powershell
$url = "https://mirrors.tuna.tsinghua.edu.cn/apache/maven/maven-3/3.9.16/binaries/apache-maven-3.9.16-bin.zip"
$zipPath = "$env:TEMP\maven.zip"
$dest = "C:\Program Files\Maven"

# 下载
Start-BitsTransfer -Source $url -Destination $zipPath -Priority High

# 解压
if (-not (Test-Path $dest)) { New-Item -ItemType Directory -Path $dest -Force | Out-Null }
Expand-Archive -Path $zipPath -DestinationPath $dest -Force
Remove-Item $zipPath -Force

# 配置环境变量
$mvnHome = "C:\Program Files\Maven\apache-maven-3.9.16"
[Environment]::SetEnvironmentVariable("MAVEN_HOME", $mvnHome, "User")
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$mvnHome\bin*") {
    [Environment]::SetEnvironmentVariable("PATH", "$mvnHome\bin;$userPath", "User")
}
```

### macOS / Linux

```bash
# 下载
wget https://mirrors.tuna.tsinghua.edu.cn/apache/maven/maven-3/3.9.16/binaries/apache-maven-3.9.16-bin.tar.gz -O /tmp/maven.tar.gz
sudo tar -C /usr/local -xzf /tmp/maven.tar.gz
sudo ln -s /usr/local/apache-maven-3.9.16 /usr/local/maven

# 配置环境变量
echo 'export MAVEN_HOME=/usr/local/maven' >> ~/.bashrc
echo 'export PATH=$MAVEN_HOME/bin:$PATH' >> ~/.bashrc
source ~/.bashrc
```

### Maven 清华源配置

```xml
<!-- ~/.m2/settings.xml -->
<settings>
  <mirrors>
    <mirror>
      <id>tuna</id>
      <mirrorOf>central</mirrorOf>
      <url>https://mirrors.tuna.tsinghua.edu.cn/maven/</url>
    </mirror>
  </mirrors>
</settings>
```

---

## Gradle 清华源配置

```groovy
// ~/.gradle/init.gradle
allprojects {
    buildscript {
        repositories {
            maven { url 'https://mirrors.tuna.tsinghua.edu.cn/maven/' }
        }
    }
    repositories {
        maven { url 'https://mirrors.tuna.tsinghua.edu.cn/maven/' }
    }
}
```

---

## 验证

```bash
java -version
javac -version
mvn -version
```
