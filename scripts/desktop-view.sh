#!/usr/bin/env bash
# 看沙箱容器的桌面（container 档位）。
#
# 用法：
#   scripts/desktop-view.sh              # 自动挑最新一个 pointer-sbx-* 容器
#   scripts/desktop-view.sh <容器名>
#
# 做了什么：
#   1. 从容器里查 VNC 端口（镜像入口已自动起 x11vnc，监听容器内 5900）
#   2. 用 macOS 自带的「屏幕共享」打开 vnc://127.0.0.1:<port>
#
# 看不到窗口时：先在里面起个程序，例如
#   docker exec <容器> sh -lc 'xterm &'      （镜像未装 xterm 时可先 apt-get install -y xterm）
set -euo pipefail

container="${1:-}"
if [ -z "$container" ]; then
    container="$(docker ps --filter 'name=pointer-sbx-' --format '{{.Names}}\t{{.CreatedAt}}' \
        | sort -k2 -r | head -1 | cut -f1)"
fi

if [ -z "$container" ]; then
    echo "没有正在运行的沙箱容器。先在 Pointer 里发一句话，让沙箱起来。" >&2
    exit 1
fi

if ! docker inspect -f '{{.State.Running}}' "$container" >/dev/null 2>&1; then
    echo "容器 $container 不存在或已停止。" >&2
    exit 1
fi

mapping="$(docker port "$container" 5900/tcp 2>/dev/null | head -1 || true)"
if [ -z "$mapping" ]; then
    echo "容器 $container 没有映射 VNC 端口（可能不是 container 档位建的）。" >&2
    echo "当前档位由 POINTER_SERVER_ENVIRONMENT_PROVIDER 决定，container 才有桌面。" >&2
    exit 1
fi

port="${mapping##*:}"
url="vnc://127.0.0.1:${port}"
password="$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$container" \
    | sed -n 's/^POINTER_VNC_PASSWORD=//p' | head -1)"

echo "容器：$container"
echo "桌面：$url"
if [ -n "$password" ]; then
    echo "密码：$password"
    if command -v pbcopy >/dev/null 2>&1; then
        printf '%s' "$password" | pbcopy
        echo "（已复制到剪贴板）"
    fi
else
    echo "密码：无"
fi

if [ "$(uname)" = "Darwin" ]; then
    open "$url"
else
    echo "用任意 VNC 客户端连 127.0.0.1:${port}（无密码）"
fi
