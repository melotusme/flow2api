#!/bin/sh
set -eu
# Run outside $HOME so Cargo does not inherit an unrelated ancestor mirror.
ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
BUILD_HOME=${FLOW2API_CARGO_HOME:-"$HOME/Library/Caches/flow2api-cargo"}
# Sign outside Documents / iCloud: File Provider can reattach FinderInfo during signing.
OUT="$HOME/Library/Caches/flow2api-local-build/Flow2API.app"
cd /private/tmp
rtk proxy env CARGO_HOME="$BUILD_HOME" cargo build --offline --manifest-path "$ROOT/desktop/Cargo.toml" -p flow2api-desktop --features tauri/custom-protocol
rtk proxy mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
rtk proxy cp "$ROOT/desktop/target/debug/flow2api-desktop" "$OUT/Contents/MacOS/flow2api-desktop"
rtk proxy cp "$ROOT/desktop/src-tauri/icons/icon.icns" "$OUT/Contents/Resources/icon.icns"
rtk proxy python3 - "$ROOT" "$OUT" <<'PY'
import json, pathlib, plistlib, sys
root, out = map(pathlib.Path, sys.argv[1:])
config = json.loads((root/'desktop/src-tauri/tauri.conf.json').read_text())
info = dict(CFBundleName='Flow2API', CFBundleDisplayName='Flow2API', CFBundleExecutable='flow2api-desktop',
    CFBundleIdentifier=config['identifier'], CFBundlePackageType='APPL', CFBundleIconFile='icon.icns',
    CFBundleShortVersionString=config['version'], CFBundleVersion=config['version'],
    LSMinimumSystemVersion='12.0', NSHighResolutionCapable=True)
(out/'Contents/Info.plist').write_bytes(plistlib.dumps(info))
PY
rtk proxy xattr -cr "$OUT"
rtk proxy codesign --force --deep --sign - "$OUT"
rtk proxy codesign --verify --deep --strict "$OUT"
rtk proxy mkdir -p "$ROOT/dist"
rtk proxy ditto -c -k --keepParent --norsrc --noextattr "$OUT" "$ROOT/dist/Flow2API_0.3.0_macOS-arm64-local.zip"
rtk proxy printf 'Built local development app: %s\n' "$OUT"
