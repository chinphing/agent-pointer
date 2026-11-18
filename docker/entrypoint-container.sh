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

# home 目录挂的是宿主目录，会把镜像里 useradd 生成的 skel 文件（.bashrc/.profile）
# 盖掉。空目录时补回来——只在缺失时写，不覆盖用户自己的文件。
if [ -d /etc/skel ]; then
    for f in /etc/skel/.[!.]*; do
        [ -e "$f" ] || continue
        [ -e "${HOME}/$(basename "$f")" ] || cp "$f" "${HOME}/" 2>/dev/null || true
    done
fi

# 浏览器 profile 的锁同理：SingletonLock 指向上一台容器的 hostname，
# 新容器里 Chromium 会认为"profile 正被另一台机器使用"并直接拒绝启动。
# 只在锁确实属于别的 hostname 时清理——同机多实例时保持原锁不动。
BROWSER_PROFILE="${BROWSER_PROFILE:-/home/pointer/.config/chromium}"
if [ -L "$BROWSER_PROFILE/SingletonLock" ]; then
    LOCK_OWNER=$(readlink "$BROWSER_PROFILE/SingletonLock" 2>/dev/null || echo "")
    LOCK_HOST=${LOCK_OWNER%-*}
    if [ -n "$LOCK_HOST" ] && [ "$LOCK_HOST" != "$(hostname)" ]; then
        rm -f "$BROWSER_PROFILE/SingletonLock" \
              "$BROWSER_PROFILE/SingletonCookie" \
              "$BROWSER_PROFILE/SingletonSocket" 2>/dev/null || true
    fi
fi

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

# 面板「接管电脑」通道：把 5900 的 RFB 包成 WebSocket，前端用 noVNC 直连。
# 只监听容器内 6080，宿主映射成 127.0.0.1 的随机端口（见 DockerEnv::run_args）。
# 失败不阻塞容器启动：接管面板连不上时会给出明确提示，其余功能不受影响。
#
# 从 /tmp 起而不是继承工作目录：websockify 启动时会 getcwd()，工作区被删掉
# （会话切换、临时目录清理）时会直接 FileNotFoundError 崩掉。
if command -v websockify >/dev/null 2>&1; then
    (cd /tmp && exec websockify --web=/usr/share/novnc 6080 127.0.0.1:5900) \
        >/tmp/websockify.log 2>&1 &
fi

"$@" &
APP_PID=$!

wait "$APP_PID"
STATUS=$?
kill "$WM_PID" "$XVFB_PID" 2>/dev/null || true
exit "$STATUS"
