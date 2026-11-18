#
# L2 执行环境层：python。
#
# 只装 python 本体（解释器 + venv + pip）和中文渲染字体，**不装任何第三方 pip 包**
# ——技能依赖由各自的 venv 承担（服务器上 `setup.sh` 建好，随技能目录挂进沙箱）。
# 所以同一个镜像可以跑任意 python 技能。
#
# 构建：
#   docker build -f docker/runtime.Dockerfile \
#     --build-arg BASE_IMAGE=pointer-sandbox-base:2026.11 \
#     -t pointer-sandbox-light:2026.11 .
#
# python 用**发行版自带的**（Ubuntu 24.04 → /usr/bin/python3.12）而不是 python.org
# 镜像：技能 venv 的 `bin/python3` 是指向 `/usr/bin/python3.x` 的符号链接，只有
# 路径和版本都对上，挂进沙箱才能用。
#
ARG BASE_IMAGE=pointer-sandbox-base:latest
FROM ${BASE_IMAGE}
USER root
ARG APT_MIRROR=mirrors.tuna.tsinghua.edu.cn

# arm64 走 ports.ubuntu.com/ubuntu-ports，x86 走 archive/security；两者都要换源，
# 否则在 Apple Silicon 上构建会一直连境外源超时。
RUN sed -i "s|ports.ubuntu.com|${APT_MIRROR}|g; \
            s|archive.ubuntu.com|${APT_MIRROR}|g; \
            s|security.ubuntu.com|${APT_MIRROR}|g; \
            s|deb.debian.org|${APT_MIRROR}|g" \
        /etc/apt/sources.list /etc/apt/sources.list.d/* 2>/dev/null || true; \
    apt-get update && apt-get install -y --no-install-recommends \
        python3 \
        python3-venv \
        python3-pip \
        fonts-noto-cjk \
 && rm -rf /var/lib/apt/lists/* \
 && python3 -V && command -v python3

# Ubuntu 24.04 的 pip 是 externally-managed：直接 `pip install` 会被拒。
# 技能一律走自己的 venv（`python3 -m venv`），不受影响；需要装到系统环境时加
# `--break-system-packages`。

USER pointer
WORKDIR /workspace
