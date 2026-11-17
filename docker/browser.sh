#!/bin/sh
# 容器里的浏览器入口，同时被安装成 /usr/bin/chromium。
#
# 为什么要替换 Debian 的 /usr/bin/chromium：那个脚本第 13 行写死了
# CHROMIUM_FLAGS=""，镜像 ENV 传进去会被重置掉，参数根本到不了浏览器。
# 替换之后，所有调用路径（直接敲 chromium、xdg-open、桌面文件）都带同一套参数。
#
# 不再需要 --no-sandbox：容器以非 root 的 pointer 用户运行，Chromium 沙箱可用。
# --user-data-dir 指向挂载的用户 home（容器以 pointer 运行，HOME=/home/pointer），
# 容器重建后登录态仍在。
#
# 窗口尺寸按当前屏幕算，铺满即可。真正的"铺满"靠 openbox 规则
# （/etc/xdg/openbox/rc.xml）：Chromium 会把上次的窗口尺寸记进 profile，
# 持久化 profile 之后 --window-size 会被记忆值覆盖，只有 WM 强制最大化才稳。
# 这里的参数是 openbox 没起来时的兜底。
SIZE=$(xdpyinfo 2>/dev/null | awk '/dimensions:/{print $2; exit}')
[ -n "$SIZE" ] || SIZE=1600x1000

exec /usr/lib/chromium/chromium \
    --disable-dev-shm-usage \
    --disable-gpu \
    --user-data-dir=/home/pointer/.config/chromium \
    --no-first-run \
    --no-default-browser-check \
    --disable-features=Translate,MediaRouter \
    --window-size="$SIZE" \
    --window-position=0,0 \
    "$@"
