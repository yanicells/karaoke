#!/usr/bin/env bash
# Build incrementally with Cargo, then validate and publish a signed native bundle.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIGURATION="${1:-release}"
case "$CONFIGURATION" in
  release|debug) ;;
  --help|-h) echo "Usage: $(basename "$0") [release|debug]"; exit 0 ;;
  *) echo "Unknown configuration: $CONFIGURATION" >&2; exit 1 ;;
esac
[[ $# -le 1 ]] || { echo "Usage: $(basename "$0") [release|debug]" >&2; exit 1; }
[[ "$(uname -s)" == Darwin ]] || { echo "KaraokeMic requires macOS." >&2; exit 1; }
cd "$ROOT_DIR"
source "$ROOT_DIR/version.env"
APP_PATH="$ROOT_DIR/dist/KaraokeMic.app"
mkdir -p "$ROOT_DIR/dist"
STAGING_DIR="$(mktemp -d "$ROOT_DIR/dist/.package.XXXXXX")"
cleanup() {
  if [[ -d "$STAGING_DIR/previous.app" && ! -e "$APP_PATH" ]]; then
    if ! mv "$STAGING_DIR/previous.app" "$APP_PATH"; then
      echo "Could not restore the previous app; preserved at $STAGING_DIR/previous.app" >&2
      return 1
    fi
  fi
  rm -rf "$STAGING_DIR"
}
trap cleanup EXIT
BUILD_ARGS=(build --locked --package karaoke-mic --bin KaraokeMic --message-format=json-render-diagnostics)
[[ "$CONFIGURATION" != release ]] || BUILD_ARGS+=(--release)
# The Cargo JSON artifact path also respects custom target directories and targets.
MACOSX_DEPLOYMENT_TARGET=14.0 cargo "${BUILD_ARGS[@]}" > "$STAGING_DIR/build.jsonl"
BINARY_PATH="$(/usr/bin/python3 - "$STAGING_DIR/build.jsonl" <<'PY'
import json, sys
binary = None
with open(sys.argv[1]) as messages:
    for line in messages:
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item["target"]["name"] == "KaraokeMic":
            binary = item.get("executable") or binary
if not binary:
    sys.exit("Cargo did not report a KaraokeMic executable.")
print(binary)
PY
)"
STAGED_APP="$STAGING_DIR/KaraokeMic.app"
mkdir -p "$STAGED_APP/Contents/MacOS" "$STAGED_APP/Contents/Resources"
cp "$BINARY_PATH" "$STAGED_APP/Contents/MacOS/KaraokeMic"
chmod +x "$STAGED_APP/Contents/MacOS/KaraokeMic"
cp "$ROOT_DIR/assets/KaraokeMic.icns" "$STAGED_APP/Contents/Resources/KaraokeMic.icns"
/usr/bin/python3 - "$STAGED_APP/Contents/Info.plist" "$MARKETING_VERSION" "$BUILD_NUMBER" <<'PY'
import plistlib, sys
info = {
    "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleName": "KaraokeMic",
    "CFBundleDisplayName": "KaraokeMic",
    "CFBundleIdentifier": "com.karaokemic.KaraokeMic",
    "CFBundleExecutable": "KaraokeMic",
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": sys.argv[2],
    "CFBundleVersion": sys.argv[3],
    "CFBundleIconFile": "KaraokeMic.icns",
    "LSApplicationCategoryType": "public.app-category.music",
    "LSMinimumSystemVersion": "14.0",
    "LSUIElement": False,
    "NSPrincipalClass": "NSApplication",
    "NSHighResolutionCapable": True,
    "NSMicrophoneUsageDescription": "KaraokeMic needs microphone access to route your voice to the speaker.",
}
with open(sys.argv[1], "wb") as destination:
    plistlib.dump(info, destination)
PY
# Local builds are unsandboxed and ad-hoc signed; no developer account is needed.
xattr -cr "$STAGED_APP"
codesign --force --sign - --identifier com.karaokemic.KaraokeMic "$STAGED_APP"
"$ROOT_DIR/Scripts/validate_bundle.sh" "$STAGED_APP"
[[ ! -e "$APP_PATH" ]] || mv "$APP_PATH" "$STAGING_DIR/previous.app"
mv "$STAGED_APP" "$APP_PATH"
echo "Created $APP_PATH ($CONFIGURATION, $(uname -m))"
