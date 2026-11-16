#!/bin/sh
# 容器里的浏览器入口。
#
# 为什么需要包装：容器以 root 跑，Chromium 的沙箱在 root 下会直接拒绝启动，
# 所以必须 --no-sandbox；容器内 /dev/shm 通常只有 64MB，不关掉共享内存会让
# 标签页随机崩溃（SIGBUS）。
exec /usr/bin/chromium \
    --no-sandbox \
    --disable-dev-shm-usage \
    --disable-gpu \
    --no-first-run \
    --no-default-browser-check \
    --disable-features=Translate,MediaRouter \
    --window-size=1600,1000 \
    --window-position=40,40 \
    "$@"
