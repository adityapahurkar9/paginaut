#!/usr/bin/env bash
# Prepares a Debian or Ubuntu machine (or WSL distribution) to build the Linux app: the system packages
# Tauri and MuPDF need, and Rust. Run as root (sudo, or wsl -u root); Rust goes to the user given as the first
# argument (default: root).
#
# Usage: sudo bash tools/linux-setup.sh [user]
set -euo pipefail

user=${1:-root}
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq \
  build-essential pkg-config curl wget file rsync patchelf desktop-file-utils \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev libssl-dev \
  xdg-utils
# Rust from rustup (the distributions' own are too old for Tauri 2).
if ! su - "$user" -c 'command -v cargo || test -x ~/.cargo/bin/cargo' >/dev/null 2>&1; then
  su - "$user" -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal'
fi
su - "$user" -c '~/.cargo/bin/rustc --version'
echo "Ready. Build with: bash tools/linux.sh release (or check / build / bundle)"
