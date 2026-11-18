#
# L1 基础层：OS + 「工具调用」依赖。
#
# 只放 agent 的 terminal / file / 搜索工具默认假定存在的东西，以及版本控制。
# 不放 python（L2）、不放 ffmpeg/字体/chromium（按场景的应用层）。
#
# 构建（一般由 docker/build.sh 调用）：
#   docker build -f docker/base.Dockerfile -t pointer-sandbox-base:2026.11 .
#
# git 用源码编译：Debian/Ubuntu 的 git 包硬依赖 perl（实测 107 MB，其中 perl 栈
# 50 MB），而 `apt-get remove perl` 会把 git 一起带走。源码编 NO_PERL 版 + strip
# 只要 23 MB，且 agent 用不到 git svn / send-email / instaweb / cvsserver。
#
ARG BASE_OS=ubuntu:24.04
ARG APT_MIRROR=mirrors.tuna.tsinghua.edu.cn
ARG GIT_VERSION=2.43.0

# ── 构建阶段：只产出 /usr/local/{bin,libexec,share}/git* ──────────────────
FROM ${BASE_OS} AS gitbuild
ARG APT_MIRROR
ARG GIT_VERSION
# arm64 走 ports.ubuntu.com/ubuntu-ports，x86 走 archive/security；两者都要换源，
# 否则在 Apple Silicon 上构建会一直连境外源超时。
RUN sed -i "s|ports.ubuntu.com|${APT_MIRROR}|g; \
            s|archive.ubuntu.com|${APT_MIRROR}|g; \
            s|security.ubuntu.com|${APT_MIRROR}|g; \
            s|deb.debian.org|${APT_MIRROR}|g" \
        /etc/apt/sources.list /etc/apt/sources.list.d/* 2>/dev/null || true; \
    apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl xz-utils \
        zlib1g-dev libssl-dev libcurl4-openssl-dev libexpat1-dev gettext \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN curl -fsSL -o git.tar.xz \
        "https://${APT_MIRROR}/ubuntu/pool/main/g/git/git_${GIT_VERSION}.orig.tar.xz" \
 && tar xf git.tar.xz && mv "git-${GIT_VERSION}" git && cd git \
 && make -j"$(nproc)" NO_PERL=YesPlease NO_TCLTK=YesPlease NO_GETTEXT=YesPlease \
        NO_PYTHON=YesPlease prefix=/usr/local all >/dev/null \
 && make NO_PERL=YesPlease NO_TCLTK=YesPlease NO_GETTEXT=YesPlease \
        NO_PYTHON=YesPlease prefix=/usr/local INSTALL_STRIP='-s' install >/dev/null \
 && git --version

# ── 运行阶段 ─────────────────────────────────────────────────────────────
FROM ${BASE_OS}
ARG APT_MIRROR
# arm64 走 ports.ubuntu.com/ubuntu-ports，x86 走 archive/security；两者都要换源，
# 否则在 Apple Silicon 上构建会一直连境外源超时。
RUN sed -i "s|ports.ubuntu.com|${APT_MIRROR}|g; \
            s|archive.ubuntu.com|${APT_MIRROR}|g; \
            s|security.ubuntu.com|${APT_MIRROR}|g; \
            s|deb.debian.org|${APT_MIRROR}|g" \
        /etc/apt/sources.list /etc/apt/sources.list.d/* 2>/dev/null || true; \
    apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        less \
        procps \
        ripgrep \
        jq \
        file \
        unzip \
        tar \
        gzip \
        xz-utils \
        p7zip-full \
        unrar-free \
        sudo \
 && rm -rf /var/lib/apt/lists/*

COPY --from=gitbuild /usr/local/bin/git /usr/local/bin/git
COPY --from=gitbuild /usr/local/libexec/git-core /usr/local/libexec/git-core
COPY --from=gitbuild /usr/local/share/git-core /usr/local/share/git-core

# 非 root 运行（对齐 Cursor 云 agent 模型：uid 1000 + sudo）。
# 技能里需要装系统包时用 sudo，避免以 root 跑 agent。
# ubuntu:24.04 自带一个 uid 1000 的 `ubuntu` 用户，先删掉再建 pointer，
# 否则 `useradd -u 1000` 报 "UID 1000 is not unique"。
RUN userdel -r ubuntu 2>/dev/null || true; \
    useradd -m -u 1000 pointer \
 && echo 'pointer ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/pointer \
 && chmod 0440 /etc/sudoers.d/pointer \
 && mkdir -p /workspace /skills && chown -R pointer:pointer /workspace /skills

ENV LANG=C.UTF-8 \
    LC_ALL=C.UTF-8
WORKDIR /workspace
USER pointer
