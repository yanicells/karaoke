#!/usr/bin/env bash
# Build incrementally with Cargo, then validate and publish a signed native bundle.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIGURATION="${1:-release}"
usage() {
  echo "Usage: $(basename "$0") [release|debug]"
  echo "Set ARCHES to one or both of: arm64 x86_64 (default: host architecture)."
}
case "$CONFIGURATION" in
  release|debug) ;;
  --help|-h) usage; exit 0 ;;
  *) echo "Unknown configuration: $CONFIGURATION" >&2; exit 1 ;;
esac
[[ $# -le 1 ]] || { usage >&2; exit 1; }
HOST_ARCH="$(uname -m)"
if [[ ${ARCHES+x} ]]; then
  [[ -n "$ARCHES" ]] || { echo "ARCHES must not be empty when explicitly set." >&2; exit 1; }
  ARCHES_INPUT="${ARCHES//$'\n'/ }"
  read -r -a ARCH_LIST <<< "$ARCHES_INPUT"
else
  ARCH_LIST=("$HOST_ARCH")
fi
[[ ${#ARCH_LIST[@]} -gt 0 ]] || { echo "ARCHES must contain arm64 and/or x86_64." >&2; exit 1; }
for ARCH in "${ARCH_LIST[@]}"; do
  case "$ARCH" in
    arm64|x86_64) ;;
    *) echo "Unsupported architecture '$ARCH'; use arm64 and/or x86_64." >&2; exit 1 ;;
  esac
  COUNT=0
  for CHECK_ARCH in "${ARCH_LIST[@]}"; do
    [[ "$CHECK_ARCH" != "$ARCH" ]] || ((COUNT += 1))
  done
  [[ "$COUNT" -eq 1 ]] || { echo "Duplicate architecture '$ARCH' in ARCHES." >&2; exit 1; }
done
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
BINARY_PATHS=()
for ARCH in "${ARCH_LIST[@]}"; do
  case "$ARCH" in
    arm64) TARGET="aarch64-apple-darwin" ;;
    x86_64) TARGET="x86_64-apple-darwin" ;;
  esac
  BUILD_ARGS_FOR_ARCH=("${BUILD_ARGS[@]}")
  [[ "$ARCH" == "$HOST_ARCH" ]] || BUILD_ARGS_FOR_ARCH+=(--target "$TARGET")
  BUILD_LOG="$STAGING_DIR/build-$ARCH.jsonl"
  if [[ "$ARCH" == "$HOST_ARCH" ]]; then
    echo "Building $CONFIGURATION for host architecture $ARCH"
  else
    echo "Building $CONFIGURATION for $ARCH ($TARGET)"
  fi
  MACOSX_DEPLOYMENT_TARGET=14.0 cargo "${BUILD_ARGS_FOR_ARCH[@]}" > "$BUILD_LOG"
  BINARY_PATH="$(/usr/bin/python3 - "$BUILD_LOG" <<'PY'
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
  [[ -f "$BINARY_PATH" ]] || { echo "Cargo executable not found: $BINARY_PATH" >&2; exit 1; }
  ACTUAL_ARCHES="$(lipo -archs "$BINARY_PATH")"
  [[ "$ACTUAL_ARCHES" == "$ARCH" ]] || {
    echo "Architecture mismatch for $BINARY_PATH (requested: $ARCH, actual: $ACTUAL_ARCHES). Check Cargo target configuration." >&2
    exit 1
  }
  BINARY_PATHS+=("$BINARY_PATH")
done
STAGED_APP="$STAGING_DIR/KaraokeMic.app"
mkdir -p "$STAGED_APP/Contents/MacOS" "$STAGED_APP/Contents/Resources"
STAGED_BINARY="$STAGED_APP/Contents/MacOS/KaraokeMic"
if [[ ${#BINARY_PATHS[@]} -gt 1 ]]; then
  lipo -create "${BINARY_PATHS[@]}" -output "$STAGED_BINARY"
else
  cp "${BINARY_PATHS[0]}" "$STAGED_BINARY"
fi
chmod +x "$STAGED_BINARY"
ACTUAL_ARCHES="$(lipo -archs "$STAGED_BINARY")"
EXPECTED_ARCHES="${ARCH_LIST[*]}"
if [[ "$(tr ' ' '\n' <<< "$ACTUAL_ARCHES" | LC_ALL=C sort | tr '\n' ' ')" != "$(tr ' ' '\n' <<< "$EXPECTED_ARCHES" | LC_ALL=C sort | tr '\n' ' ')" ]]; then
  echo "Packaged executable architecture mismatch (expected: $EXPECTED_ARCHES, actual: $ACTUAL_ARCHES)." >&2
  exit 1
fi
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
echo "Created $APP_PATH ($CONFIGURATION, ${ARCH_LIST[*]})"
