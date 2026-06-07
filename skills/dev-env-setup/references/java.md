# Java 安装指南（JDK 21 LTS）

## macOS

### 华为镜像下载安装

```bash
# 下载 JDK 21 华为镜像（arm64 / x64 按需选择）
arch_name="arm64"  # Apple Silicon
# arch_name="x64"  # Intel
wget https://mirrors.huaweicloud.com/eclipse/adoptium/jdk/21.0.6%2B7/OpenJDK21U-jdk_${arch_name}_mac_hotspot_21.0.6_7.tar.gz -O /tmp/jdk21.tar.gz

# 解压安装
sudo tar -C /Library/Java/JavaVirtualMachines -xzf /tmp/jdk21.tar.gz
rm /tmp/jdk21.tar.gz

# 设置 JAVA_HOME
echo 'export JAVA_HOME=$(/usr/libexec/java_home -v 21)' >> ~/.zshrc
echo 'export PATH=$JAVA_HOME/bin:$PATH' >> ~/.zshrc
source ~/.zshrc
```

## Linux

### 华为镜像下载安装

```bash
wget https://mirrors.huaweicloud.com/eclipse/adoptium/jdk/21.0.6+7/OpenJDK21U-jdk_x64_linux_hotspot_21.0.6_7.tar.gz -O /tmp/jdk21.tar.gz
sudo tar -C /usr/local -xzf /tmp/jdk21.tar.gz
sudo mv /usr/local/jdk-21.0.6+7 /usr/local/jdk-21
echo 'export JAVA_HOME=/usr/local/jdk-21' >> ~/.bashrc
echo 'export PATH=$JAVA_HOME/bin:$PATH' >> ~/.bashrc
source ~/.bashrc
```

## Windows

### 华为镜像下载安装

```powershell
# 下载 JDK 21
$url = "https://mirrors.huaweicloud.com/eclipse/adoptium/jdk/21.0.6+7/OpenJDK21U-jdk_x64_windows_hotspot_21.0.6_7.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\jdk21.zip"
Expand-Archive -Path "$env:TEMP\jdk21.zip" -DestinationPath "C:\Program Files\Java" -Force

# 配置环境变量
[Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Java\jdk-21.0.6+7", "Machine")
$path = [Environment]::GetEnvironmentVariable("Path", "Machine")
[Environment]::SetEnvironmentVariable("Path", "$path;%JAVA_HOME%\bin", "Machine")
```

或手动：
1. 打开 `https://mirrors.huaweicloud.com/eclipse/adoptium/jdk/21.0.6+7/`
2. 下载 `OpenJDK21U-jdk_x64_windows_hotspot_21.0.6_7.zip`
3. 解压到 `C:\Program Files\Java\`
4. 设置系统环境变量 `JAVA_HOME` + `PATH`

---

## Maven 华为镜像

```xml
<!-- ~/.m2/settings.xml -->
<mirrors>
  <mirror>
    <id>huawei</id>
    <mirrorOf>central</mirrorOf>
    <url>https://mirrors.huaweicloud.com/repository/maven/</url>
  </mirror>
</mirrors>
```

## Gradle 华为镜像

```groovy
// ~/.gradle/init.gradle
allprojects {
    buildscript {
        repositories {
            maven { url 'https://mirrors.huaweicloud.com/repository/maven/' }
        }
    }
    repositories {
        maven { url 'https://mirrors.huaweicloud.com/repository/maven/' }
    }
}
```

## 验证

```bash
java -version
javac -version
```
