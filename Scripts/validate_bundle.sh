#!/usr/bin/env bash
# Static validation only: never launch the app or request microphone permission.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH="${1:-$ROOT_DIR/dist/KaraokeMic.app}"
[[ $# -le 1 ]] || { echo "Usage: $(basename "$0") [app-path]" >&2; exit 1; }
PLIST="$APP_PATH/Contents/Info.plist"
fail() { echo "Invalid app bundle: $*" >&2; exit 1; }
[[ -f "$PLIST" ]] || fail "missing Info.plist"
plutil -lint "$PLIST" >/dev/null
require_value() {
  local actual
  actual="$(/usr/libexec/PlistBuddy -c "Print :$1" "$PLIST")"
  [[ "$actual" == "$2" ]] || fail "$1 must be $2 (got $actual)"
}
require_value CFBundleIdentifier com.karaokemic.KaraokeMic
require_value CFBundleExecutable KaraokeMic
require_value CFBundlePackageType APPL
require_value LSMinimumSystemVersion 14.0
require_value NSPrincipalClass NSApplication
require_value LSUIElement false
require_value NSHighResolutionCapable true
require_value NSMicrophoneUsageDescription "KaraokeMic needs microphone access to route your voice to the speaker."
require_value CFBundleIconFile KaraokeMic.icns
[[ -s "$APP_PATH/Contents/Resources/KaraokeMic.icns" ]] || fail "missing icon"
BINARY="$APP_PATH/Contents/MacOS/KaraokeMic"
[[ -x "$BINARY" ]] || fail "missing executable"
file "$BINARY" | /usr/bin/grep -q 'Mach-O' || fail "executable is not a native Mach-O binary"
codesign --verify --strict "$APP_PATH"
echo "Validated $APP_PATH"
