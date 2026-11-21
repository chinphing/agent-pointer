#
# L3 桌面层（desktop 档位）：headless 的全部能力 + 图形桌面。
#
# 构建：
#   docker build -f docker/desktop.Dockerfile \
#     --build-arg BASE_IMAGE=pointer-sandbox-headless:2026.11 \
#     -t pointer-sandbox-desktop:2026.11 .
#
# 桌面怎么起来（入口脚本负责）：
#   - Xvfb 在 :99 上开 1920x1080x24 虚拟屏
#   - openbox 做窗口管理器（没有 WM 时窗口没标题栏、无法聚焦，截图和点击都会失真）
#   - x11vnc + websockify 给「接管电脑」面板用
#
# 不放 ffmpeg：拖整套编解码库 +418 MB，只有媒体类技能需要。
#
ARG BASE_IMAGE=pointer-sandbox-headless:latest
FROM ${BASE_IMAGE}
USER root
ARG APT_MIRROR=mirrors.tuna.tsinghua.edu.cn

ENV DEBIAN_FRONTEND=noninteractive \
    DISPLAY=:99 \
    SCREEN_WIDTH=1920 \
    SCREEN_HEIGHT=1080 \
    SCREEN_DEPTH=24

RUN sed -i "s|ports.ubuntu.com|${APT_MIRROR}|g; \
            s|archive.ubuntu.com|${APT_MIRROR}|g; \
            s|security.ubuntu.com|${APT_MIRROR}|g" \
        /etc/apt/sources.list /etc/apt/sources.list.d/* 2>/dev/null || true; \
    apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 update \
 && apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 install -y --no-install-recommends \
        xvfb \
        x11-utils \
        x11-xserver-utils \
        x11vnc \
        websockify \
        novnc \
        xdotool \
        openbox \
        pcmanfm \
        plank \
        imagemagick \
        scrot \
        dbus-x11 \
        xclip \
        xterm \
        poppler-utils \
        fonts-dejavu \
        fonts-liberation \
        adwaita-icon-theme \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix

# 右键菜单 + 窗口规则：空桌面上唯一能点的地方，也给浏览器铺满屏幕的规则。
COPY docker/desktop/ /etc/pointer/desktop/
COPY docker/openbox-menu.xml /etc/xdg/openbox/menu.xml
COPY docker/openbox-rc.xml /etc/xdg/openbox/rc.xml

# 浏览器：Ubuntu 24.04 的 `chromium` 是 snap 占位包，deb 里没有真正的二进制
# （实测 `/usr/lib/chromium/chromium: not found`）。改用 Google Chrome 官方 deb，
# 由 browser.sh 以 `--class=chromium` 启动，X 里的窗口 class 仍是 chromium。
# 浏览器：Ubuntu 24.04 的 `chromium` 是 snap 占位包，deb 里没有真正的二进制
# （实测 `/usr/lib/chromium/chromium: not found`）。改用 Google Chrome 官方 deb，
# 由 browser.sh 以 `--class=chromium` 启动，X 里的窗口 class 仍是 chromium。
#
# deb 由 `docker/build.sh` 预先下到宿主再喂进来（命名构建上下文 `chrome`）：
# 构建容器到 dl.google.com 的 TLS 连接在国内经常 35/18 失败，宿主侧重试可控。
# 手工构建时：`--build-context chrome=<放 deb 的目录>`。
# 桌面文件（dock / 右键菜单）里写的是 Icon=/usr/share/pixmaps/chromium.png，
# 旧镜像由 chromium 包提供；换成 Chrome 后这个路径不存在，plank 会把该项丢掉，
# 所以补一个指向 Chrome logo 的符号链接。
#
# deb 用 bind mount 挂进来而不是 COPY：COPY 会在镜像里留一层 142 MB 的文件，
# 后面 `rm` 只删得掉同一层里的副本，COPY 那层照样占空间。
RUN --mount=type=bind,from=chrome,source=google-chrome.deb,target=/tmp/chrome.deb \
    apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 update \
 && apt-get -o Acquire::Retries=3 install -y --no-install-recommends /tmp/chrome.deb \
 && rm -rf /var/lib/apt/lists/* \
 && /opt/google/chrome/chrome --version \
 && mkdir -p /usr/share/pixmaps \
 && ln -sf /opt/google/chrome/product_logo_256.png /usr/share/pixmaps/chromium.png

# 覆盖 /usr/bin/chromium，另给一个好记的名字 /usr/local/bin/browser。
COPY docker/browser.sh /usr/bin/chromium
RUN chmod +x /usr/bin/chromium \
 && ln -sf /usr/bin/chromium /usr/local/bin/browser

COPY docker/entrypoint-container.sh /usr/local/bin/entrypoint-container.sh
RUN chmod +x /usr/local/bin/entrypoint-container.sh

USER pointer
WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/entrypoint-container.sh"]
CMD ["sleep", "infinity"]
