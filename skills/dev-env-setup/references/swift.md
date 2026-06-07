# Swift 安装指南

## macOS（官方主平台）

### 通过 Xcode 安装

```bash
xcode-select --install
swift --version
```

Xcode 安装完成后自动包含 Swift 编译器终端支持。

### 单独 Swift Toolchain（无需完整 Xcode）

从华为镜像下载（如可用）或清华镜像：

```bash
# 华为镜像（如果 mirrors.huaweicloud.com 有 Swift）
# 或使用清华镜像
wget https://mirrors.tuna.tsinghua.edu.cn/swift-server/swift-6.0-release/xcode/swift-6.0-RELEASE/swift-6.0-RELEASE-osx.pkg
# 双击安装 pkg
```

## Linux

```bash
# Ubuntu — 清华镜像
version="6.0-RELEASE"
wget https://mirrors.tuna.tsinghua.edu.cn/swift-server/swift-${version}/ubuntu2404/swift-${version}-ubuntu24.04.tar.gz -O /tmp/swift.tar.gz
sudo tar -C /usr/local -xzf /tmp/swift.tar.gz
echo 'export PATH=$PATH:/usr/local/swift-${version}-ubuntu24.04/usr/bin' >> ~/.bashrc
source ~/.bashrc
```

## Windows

Swift on Windows 仍属实验性，推荐使用 **WSL2** + Linux 方案，或直接使用 macOS 开发。

## 国内镜像源参考

| 资源 | 镜像地址 |
|------|---------|
| Swift 二进制 | `https://mirrors.tuna.tsinghua.edu.cn/swift-server/` |
| 华为镜像（如果支持） | `https://mirrors.huaweicloud.com/swift/` |

## 验证

```bash
swift --version
```
