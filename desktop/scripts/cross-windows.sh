#!/usr/bin/env bash
# Build an unsigned Windows x64 installer on macOS with cargo-xwin + NSIS.
set -euo pipefail

desktop_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cross_runner="${FLOW2API_CARGO_XWIN:-cargo-xwin}"
if command -v brew >/dev/null 2>&1; then
  llvm_prefix="$(brew --prefix llvm)"
  export PATH="$llvm_prefix/bin:$PATH"
fi
for tool in cargo "$cross_runner" clang-cl lld-link llvm-rc makensis shasum; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing build tool: $tool. See docs/windows-cross-build.md." >&2
    exit 1
  fi
done

# Avoid reading unrelated ancestor Cargo configuration during cross compilation.
cd "${TMPDIR:-/tmp}"
"$cross_runner" build --manifest-path "$desktop_root/Cargo.toml" \
  -p flow2api-desktop --release --target x86_64-pc-windows-msvc --locked "$@"
cd "$desktop_root"
# Config accepts NSIS on a cross host; the CLI --bundles choices are host-specific.
cargo tauri bundle --target x86_64-pc-windows-msvc \
  --config '{"bundle":{"targets":["nsis"]}}' --ci --no-sign

mkdir -p "$desktop_root/../dist"
found=false
for installer in "$desktop_root"/target/x86_64-pc-windows-msvc/release/bundle/nsis/*-setup.exe; do
  if [[ -f "$installer" ]]; then
    cp "$installer" "$desktop_root/../dist/"
    (
      cd "$desktop_root/../dist"
      shasum -a 256 "$(basename "$installer")" > "$(basename "$installer").sha256"
    )
    echo "Installer: $desktop_root/../dist/$(basename "$installer")"
    found=true
  fi
done
if [[ "$found" != true ]]; then
  echo "NSIS did not produce an installer." >&2
  exit 1
fi
