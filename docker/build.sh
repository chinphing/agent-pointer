#!/usr/bin/env bash
#
# 构建沙箱镜像。
#
#   ./docker/build.sh                 # 全部（base → light → desktop）
#   ./docker/build.sh base            # 只重建基础层
#   ./docker/build.sh light           # 只重建执行环境层（base 用现有 tag）
#   ./docker/build.sh desktop         # 只重建桌面层（light 用现有 tag）
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
DESKTOP_IMAGE="pointer-sandbox-desktop:${VERSION}"
# container 档位的老名字：测试与 dev 配置都用它，保持别名，避免一次性改十几处。
CONTAINER_ALIAS="pointer-sandbox-container:latest"
APT_MIRROR="${APT_MIRROR:-mirrors.tuna.tsinghua.edu.cn}"

echo "version=${VERSION} apt_mirror=${APT_MIRROR}"

build_base() {
    echo "==> ${BASE_IMAGE}"
    docker build \
        -f docker/base.Dockerfile \
        --build-arg "APT_MIRROR=${APT_MIRROR}" \
        -t "${BASE_IMAGE}" -t pointer-sandbox-base:latest .
}

build_light() {
    echo "==> ${LIGHT_IMAGE}（from ${BASE_IMAGE}）"
    docker build \
        -f docker/runtime.Dockerfile \
        --build-arg "BASE_IMAGE=${BASE_IMAGE}" \
        --build-arg "APT_MIRROR=${APT_MIRROR}" \
        -t "${LIGHT_IMAGE}" -t pointer-sandbox-light:latest .
}

# Chrome deb：构建容器到 dl.google.com 的 TLS 经常失败，改在宿主预下载，
# 再用命名构建上下文喂给镜像（`COPY --from=chrome`）。
CHROME_CACHE="${POINTER_CHROME_CACHE:-${TMPDIR:-/tmp}/pointer-chrome-deb}"

chrome_deb() {
    case "$(uname -m)" in
        arm64|aarch64) _arch=arm64 ;;
        *)             _arch=amd64 ;;
    esac
    mkdir -p "${CHROME_CACHE}"
    local deb="${CHROME_CACHE}/google-chrome.deb"
    if [ ! -s "${deb}" ]; then
        echo "下载 Chrome deb（${_arch}）→ ${deb}" >&2
        curl -fL --retry 5 --retry-delay 3 --retry-all-errors \
            -o "${deb}.part" \
            "https://dl.google.com/linux/direct/google-chrome-stable_current_${_arch}.deb"
        mv "${deb}.part" "${deb}"
    fi
    echo "${CHROME_CACHE}"
}

build_desktop() {
    local ctx
    ctx="$(chrome_deb)"
    echo "==> ${DESKTOP_IMAGE}（from ${LIGHT_IMAGE}）"
    docker build \
        -f docker/desktop.Dockerfile \
        --build-arg "BASE_IMAGE=${LIGHT_IMAGE}" \
        --build-arg "APT_MIRROR=${APT_MIRROR}" \
        --build-context "chrome=${ctx}" \
        -t "${DESKTOP_IMAGE}" \
        -t pointer-sandbox-desktop:latest \
        -t "${CONTAINER_ALIAS}" .
}

targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(base light desktop)

for t in "${targets[@]}"; do
    case "$t" in
        base)  build_base ;;
        light)   build_light ;;
        desktop) build_desktop ;;
        all)     build_base; build_light; build_desktop ;;
        *) echo "未知目标：$t（可选 base / light / desktop / all）" >&2; exit 2 ;;
    esac
done

echo
docker images --format '{{.Repository}}:{{.Tag}}\t{{.Size}}' \
    | grep -E "pointer-sandbox-(base|light|desktop):${VERSION}|pointer-sandbox-container:latest" || true
