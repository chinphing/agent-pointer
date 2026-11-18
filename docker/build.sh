#!/usr/bin/env bash
#
# 构建沙箱镜像。
#
#   ./docker/build.sh                 # 全部（base → light）
#   ./docker/build.sh base            # 只重建基础层
#   ./docker/build.sh light           # 只重建执行环境层（base 用现有 tag）
#   POINTER_SANDBOX_VERSION=2026.12 ./docker/build.sh
#
# 版本进 tag：镜像名一变，Pointer 的规格指纹就变，已有容器会自动重建，
# 不需要再手动 bump SPEC_VERSION。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${POINTER_SANDBOX_VERSION:-$(date +%Y.%m)}"
BASE_IMAGE="pointer-sandbox-base:${VERSION}"
LIGHT_IMAGE="pointer-sandbox-light:${VERSION}"
APT_MIRROR="${APT_MIRROR:-mirrors.tuna.tsinghua.edu.cn}"

echo "version=${VERSION} apt_mirror=${APT_MIRROR}"

build_base() {
    echo "==> ${BASE_IMAGE}"
    docker build \
        -f docker/base.Dockerfile \
        --build-arg "APT_MIRROR=${APT_MIRROR}" \
        -t "${BASE_IMAGE}" .
}

build_light() {
    echo "==> ${LIGHT_IMAGE}（from ${BASE_IMAGE}）"
    docker build \
        -f docker/runtime.Dockerfile \
        --build-arg "BASE_IMAGE=${BASE_IMAGE}" \
        --build-arg "APT_MIRROR=${APT_MIRROR}" \
        -t "${LIGHT_IMAGE}" .
}

targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(base light)

for t in "${targets[@]}"; do
    case "$t" in
        base)  build_base ;;
        light) build_light ;;
        all)   build_base; build_light ;;
        *) echo "未知目标：$t（可选 base / light / all）" >&2; exit 2 ;;
    esac
done

echo
docker images --format '{{.Repository}}:{{.Tag}}\t{{.Size}}' \
    | grep -E "pointer-sandbox-(base|light):${VERSION}" || true
