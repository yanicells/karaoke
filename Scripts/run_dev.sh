#!/usr/bin/env bash
# Builds KaraokeMic and opens it as a .app bundle with mic permission in Info.plist.
set -euo pipefail

APP_NAME="KaraokeMic"
APP_PATH="/tmp/${APP_NAME}.app"
CONTENTS="${APP_PATH}/Contents"

echo "==> Building..."
swift build -c debug

echo "==> Assembling bundle at ${APP_PATH}..."
rm -rf "${APP_PATH}"
mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Resources"
cp ".build/debug/${APP_NAME}" "${CONTENTS}/MacOS/${APP_NAME}"
chmod +x "${CONTENTS}/MacOS/${APP_NAME}"

cat > "${CONTENTS}/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>KaraokeMic</string>
    <key>CFBundleIdentifier</key>
    <string>com.example.KaraokeMic</string>
    <key>CFBundleExecutable</key>
    <string>KaraokeMic</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>KaraokeMic needs microphone access to route your voice to the speaker.</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

echo "==> Signing ad-hoc..."
codesign --force --sign - "${APP_PATH}"

echo "==> Killing any previous instance..."
pkill -x "${APP_NAME}" 2>/dev/null || true

echo "==> Launching..."
open "${APP_PATH}"
