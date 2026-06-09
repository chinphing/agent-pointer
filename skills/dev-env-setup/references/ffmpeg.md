# ffmpeg / ffprobe 安装

Pointer 处理 IM 视频、部分音频转码依赖本机 **ffmpeg** 与 **ffprobe**。
安装包内不包含这两个工具，请按操作系统安装。

## 安装前检测

```bash
command -v ffmpeg && command -v ffprobe && ffmpeg -version | head -1
```

Windows PowerShell：

```powershell
where.exe ffmpeg
where.exe ffprobe
```

## macOS

推荐 Homebrew：

```bash
brew install ffmpeg
```

无 Homebrew 时，可从 https://ffmpeg.org/download.html 下载构建包，
将 `bin` 目录加入 `PATH`。

## Linux

Debian / Ubuntu：

```bash
sudo apt update && sudo apt install -y ffmpeg
```

Fedora：

```bash
sudo dnf install -y ffmpeg
```

Arch：

```bash
sudo pacman -S ffmpeg
```

## Windows

推荐 winget（需用户确认 UAC）：

```powershell
winget install --id Gyan.FFmpeg -e --accept-source-agreements --accept-package-agreements
```

或 Chocolatey：

```powershell
choco install ffmpeg -y
```

安装后**新开终端**再验证。若仍找不到，将安装目录下的 `bin`
（例如 `C:\ffmpeg\bin`）加入系统 PATH。

## 安装后验证

```bash
ffmpeg -version
ffprobe -version
```

验证通过后，请用户重新发送视频消息，或说「重试上一条视频」。

## 注意事项

- Linux `apt install` 可能需要 sudo，安装前先说明。
- 企业环境若禁止安装，告知用户联系 IT 或使用有 ffmpeg 的服务器部署 pointer-server。
- Web 纯浏览器端无法在本机安装；应在运行 pointer-server 的机器上安装。
