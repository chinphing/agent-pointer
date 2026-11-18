#
# Container 档位的沙箱镜像：light 的全部工具 + 图形桌面（Xvfb + WM）。
#
# 构建：
#   docker build -f docker/sandbox-container.Dockerfile -t pointer-sandbox-container:latest .
#
# 使用（Pointer 侧配置）：
#   POINTER_SERVER_ENVIRONMENT_PROVIDER=container
#   POINTER_SERVER_ENVIRONMENT_BACKEND=docker
#   POINTER_SERVER_ENVIRONMENT_TEMPLATE=pointer-sandbox-container:latest
#
# 桌面怎么起来的：
#   - Xvfb 在 :99 上跑一个 1920x1080x24 的虚拟屏
#   - openbox 做窗口管理器（没有 WM 时窗口没有标题栏/无法聚焦，截图和点击都会失真）
#   - 入口脚本先把这两个拉起来，再常驻，容器就绪时 DISPLAY 已可用
#
# 桌面工具（agent 侧用）：
#   - xdotool       → 鼠标/键盘事件
#   - x11-utils     → xdpyinfo / xwininfo（探测与调试）
#   - imagemagick   → import 截图
#   - scrot         → 轻量截图（备用）
#   - x11vnc        → 需要人工接管时看实时画面
# Node 从官方镜像整段拷过来：不再联网跑 NodeSource，
# 那一步在国内网络下经常 SSL 中断，且每次改 apt 都会失效重下。
FROM node:24-bookworm-slim AS node

FROM python:3.12-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    DISPLAY=:99 \
    SCREEN_WIDTH=1920 \
    SCREEN_HEIGHT=1080 \
    SCREEN_DEPTH=24

# 构建容器到 deb.debian.org 的连接在本机经常失败（宿主能通、buildkit 里不通），
# 换国内镜像源，和 Node 那步同一个理由。
# apt 源可覆盖：默认阿里云（国内快）。境外构建或镜像临时抽风时用
# `--build-arg APT_MIRROR=mirrors.tuna.tsinghua.edu.cn` 换一个。
ARG APT_MIRROR=mirrors.aliyun.com
RUN sed -i "s|deb.debian.org|${APT_MIRROR}|g; s|security.debian.org|${APT_MIRROR}|g" \
        /etc/apt/sources.list.d/debian.sources 2>/dev/null || true \
    && apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        gnupg \
        git \
        less \
        procps \
        ripgrep \
        ffmpeg \
        poppler-utils \
        fonts-noto-cjk \
        fonts-dejavu \
        unzip \
        tar \
        gzip \
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
        chromium \
        fonts-liberation \
        sudo \
    && rm -rf /var/lib/apt/lists/*

COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -sf /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -sf /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && node -v && npm -v && python3 -V

# 以非 root 用户运行（对齐 Cursor 云 agent 的模型：uid 1000 + sudo）。
# 好处：Chromium 沙箱可用（root 下必须 --no-sandbox），日常操作最小权限。
RUN useradd -m -u 1000 pointer \
    && echo 'pointer ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/pointer \
    && chmod 0440 /etc/sudoers.d/pointer \
    && mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix

# 右键菜单：空桌面上唯一能点的地方，给人和 agent 一个入口。
# 桌面图标 + 壁纸配置：入口脚本首次启动铺到用户 home（之后尊重用户改动）。
COPY docker/desktop/ /etc/pointer/desktop/

COPY docker/openbox-menu.xml /etc/xdg/openbox/menu.xml

# 窗口规则：浏览器铺满屏幕（见文件头注释——profile 会记住旧尺寸，覆盖 --window-size）。
COPY docker/openbox-rc.xml /etc/xdg/openbox/rc.xml

# 覆盖 Debian 的 /usr/bin/chromium（原脚本会重置 CHROMIUM_FLAGS），
# 再给一个好记的名字 /usr/local/bin/browser。
COPY docker/browser.sh /usr/bin/chromium
RUN chmod +x /usr/bin/chromium \
    && ln -sf /usr/bin/chromium /usr/local/bin/browser

COPY docker/entrypoint-container.sh /usr/local/bin/entrypoint-container.sh
RUN chmod +x /usr/local/bin/entrypoint-container.sh

WORKDIR /workspace

USER pointer

ENTRYPOINT ["/usr/local/bin/entrypoint-container.sh"]
CMD ["sleep", "infinity"]
