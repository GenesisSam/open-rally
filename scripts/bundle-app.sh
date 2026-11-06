#!/usr/bin/env bash
# release 빌드를 dist/open-rally.app 으로 싼다 (Dock 아이콘 없는 메뉴바 앱).
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release

APP=dist/open-rally.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/release/open-rally "$APP/Contents/MacOS/open-rally"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>open-rally</string>
  <key>CFBundleDisplayName</key><string>open-rally</string>
  <key>CFBundleIdentifier</key><string>dev.satune.open-rally</string>
  <key>CFBundleExecutable</key><string>open-rally</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "만들었습니다: $APP"
echo "실행: open $APP"
