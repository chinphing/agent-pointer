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
exec /usr/lib/chromium/chromium \
    --disable-dev-shm-usage \
    --disable-gpu \
    --user-data-dir=/home/pointer/.config/chromium \
    --no-first-run \
    --no-default-browser-check \
    --disable-features=Translate,MediaRouter \
    --window-size=1600,1000 \
    --window-position=40,40 \
    "$@"
