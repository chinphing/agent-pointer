#
# Light 档位的沙箱镜像：把"agent 可触发的工具"预装齐，避免运行期才发现命令不存在。
#
# 构建：
#   docker build -f docker/sandbox-light.Dockerfile -t pointer-sandbox-light:latest .
#
# 使用（Pointer 侧配置）：
#   POINTER_SERVER_ENVIRONMENT_PROVIDER=light
#   POINTER_SERVER_ENVIRONMENT_BACKEND=docker
#   POINTER_SERVER_ENVIRONMENT_TEMPLATE=pointer-sandbox-light:latest
#
# 工具链版本（2026-11 定）：Node 24、Python 3.12。
# Debian bookworm 自带的 python3 是 3.11、nodejs 是 18，所以这里从官方镜像
# `python:3.12-slim-bookworm` 起，再补 Node 24。
#
# 包含的能力：
#   - ffmpeg / ffprobe   → 视频抽帧、音频转写、faststart
#   - poppler-utils      → pdftoppm / pdfinfo / pdftotext
#   - fonts-noto-cjk     → 中文 PDF 页图不变豆腐块
#   - git / ripgrep      → 代码类工具与能力探测
#   - node 24 / python 3.12 → MCP server（stdio）运行环境
# Node 从官方镜像整段拷过来：不再联网跑 NodeSource，
# 那一步在国内网络下经常 SSL 中断，且每次改 apt 都会失效重下。
FROM node:24-bookworm-slim AS node

FROM python:3.12-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g; s|security.debian.org|mirrors.aliyun.com|g' \
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
        unzip \
        tar \
        gzip \
        sudo \
    && rm -rf /var/lib/apt/lists/*

# Node 24（NodeSource 官方源，支持 Debian bookworm / arm64）
COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -sf /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -sf /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && node -v && npm -v && python3 -V

# 以非 root 用户运行（对齐 Cursor 云 agent 的模型：uid 1000 + sudo）。
# Cube 上可映射到 uid ≥ 100000；docker 后端默认就是这个用户。
RUN useradd -m -u 1000 pointer \
    && echo 'pointer ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/pointer \
    && chmod 0440 /etc/sudoers.d/pointer

WORKDIR /workspace

USER pointer

CMD ["sleep", "infinity"]
