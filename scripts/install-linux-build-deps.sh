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
  libglib2.0-dev \
  build-essential \
  pkg-config \
  curl \
  wget \
  file \
  libfuse2 \
  squashfs-tools \
  patchelf \
  zsync \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  libgtk-3-dev \
  libgtk-3-bin \
  libgdk-pixbuf-2.0-dev \
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

missing=0
for pkg in glib-2.0 gtk+-3.0 gdk-pixbuf-2.0 librsvg-2.0; do
  if pkg-config --exists "$pkg" 2>/dev/null; then
    echo "ok: $pkg"
  else
    echo "install-linux-build-deps.sh: pkg-config missing $pkg" >&2
    missing=1
  fi
done
if command -v gtk-query-immodules-3.0 >/dev/null 2>&1; then
  echo "ok: gtk-query-immodules-3.0"
else
  echo "install-linux-build-deps.sh: gtk-query-immodules-3.0 not in PATH (libgtk-3-bin)" >&2
  missing=1
fi
if [ "$missing" -ne 0 ]; then
  echo "Some AppImage (linuxdeploy gtk plugin) prerequisites are still missing." >&2
  exit 1
fi

echo "Linux build dependencies installed."
