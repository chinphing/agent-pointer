#!/usr/bin/env bash
# System packages required for Tauri Linux deb + AppImage builds (Ubuntu/Debian).
set -euo pipefail

if ! command -v apt-get >/dev/null 2>&1; then
  echo "install-linux-build-deps.sh: apt-get not found (Ubuntu/Debian only)." >&2
  echo "Install equivalents manually: libfuse2, patchelf, file, WebKitGTK 4.1 dev, GStreamer plugins." >&2
  exit 1
fi

sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  pkg-config \
  curl \
  wget \
  file \
  libfuse2 \
  patchelf \
  zsync \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-libav \
  libpipewire-0.3-dev \
  libspa-0.2-dev \
  libclang-dev \
  libgbm-dev \
  libegl1-mesa-dev \
  libdrm-dev \
  libwayland-dev

echo "Linux build dependencies installed."
