#!/usr/bin/env bash
# Build the Windows exe in WSL, copy it out of the Linux filesystem and launch it on Windows.
# Usage: ./run.sh [--release] [-- app args, e.g. --fps 30]; runs windowed (--window), not as the wallpaper
set -euo pipefail
cd "$(dirname "$0")"
profile=debug; build_args=()
if [[ "${1:-}" == "--release" ]]; then profile=release; build_args=(--release); shift; fi
[[ "${1:-}" == "--" ]] && shift
dest=/mnt/c/Users/d/pixel3d
taskkill.exe /IM meadow.exe /F >/dev/null 2>&1 || true # a running exe is locked; the copy would fail
cargo build --target x86_64-pc-windows-gnu "${build_args[@]}"
mkdir -p "$dest"
cp "target/x86_64-pc-windows-gnu/$profile/meadow.exe" "$dest/"
cd "$dest" && exec ./meadow.exe --window "$@"
