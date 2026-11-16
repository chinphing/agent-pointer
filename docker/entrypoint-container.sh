#!/bin/sh
# Container 档位入口：先把虚拟桌面拉起来，再执行 CMD。
#
# 为什么不用 supervisor：这里只有两个常驻进程，且失败模式很单一
# （Xvfb 起不来 → DISPLAY 不可用）。用 sh 直接起、起不来就报错退出，
# 比引入进程管理器更好排查。
set -eu

WIDTH="${SCREEN_WIDTH:-1920}"
HEIGHT="${SCREEN_HEIGHT:-1080}"
DEPTH="${SCREEN_DEPTH:-24}"
DISPLAY_NUM="${DISPLAY#:}"
DISPLAY_NUM="${DISPLAY_NUM%%.*}"

# X 的锁文件残留会让 Xvfb 直接退出（容器重启后常见）。
rm -f "/tmp/.X${DISPLAY_NUM}-lock" "/tmp/.X11-unix/X${DISPLAY_NUM}" 2>/dev/null || true

Xvfb ":${DISPLAY_NUM}" -screen 0 "${WIDTH}x${HEIGHT}x${DEPTH}" -nolisten tcp &
XVFB_PID=$!

# 等 X 就绪（最多 10s），避免 WM 抢跑失败。
i=0
while [ $i -lt 50 ]; do
    if xdpyinfo -display ":${DISPLAY_NUM}" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$XVFB_PID" 2>/dev/null; then
        echo "entrypoint: Xvfb 启动失败" >&2
        exit 1
    fi
    i=$((i + 1))
    sleep 0.2
done

DISPLAY=":${DISPLAY_NUM}" openbox &
WM_PID=$!

# 两个常驻进程任一退出就整体退出，交给容器重启策略。
trap 'kill "$WM_PID" "$XVFB_PID" 2>/dev/null || true' TERM INT

# 根窗口默认纯黑，连上去像"坏了"。给个深灰底色，一眼能看出桌面是活的。
# 必须在 openbox 之后设：openbox 启动时会重置根窗口，先设会被清回黑色。
sleep 0.5
xsetroot -display ":${DISPLAY_NUM}" -solid "#1f2430" 2>/dev/null || true

# 桌面观察通道：VNC 只监听容器内 5900，宿主机通过端口映射访问（见 DockerEnv）。
# 不开密码：容器只绑 127.0.0.1 的随机宿主端口，等价于本机可见。
# 密码由 DockerEnv 每会话随机生成，通过环境变量传进来。
# 没传就退回无密码（只绑 127.0.0.1 的随机端口，本机可见）。
if command -v x11vnc >/dev/null 2>&1; then
    if [ -n "${POINTER_VNC_PASSWORD:-}" ]; then
        x11vnc -storepasswd "$POINTER_VNC_PASSWORD" /tmp/.vncpass >/dev/null 2>&1 || true
        AUTH_ARGS="-rfbauth /tmp/.vncpass"
    else
        AUTH_ARGS="-nopw"
    fi
    # shellcheck disable=SC2086
    x11vnc -display ":${DISPLAY_NUM}" -forever $AUTH_ARGS -shared -rfbport 5900 \
        -bg -o /tmp/x11vnc.log >/dev/null 2>&1 || true
fi

"$@" &
APP_PID=$!

wait "$APP_PID"
STATUS=$?
kill "$WM_PID" "$XVFB_PID" 2>/dev/null || true
exit "$STATUS"
