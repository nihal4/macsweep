#!/bin/bash
# Builds an optimised binary and wraps it into dist/MacSweep.app
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

APP="dist/MacSweep.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/macsweep "$APP/Contents/MacOS/MacSweep"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>MacSweep</string>
    <key>CFBundleIdentifier</key><string>com.nihal.macsweep</string>
    <key>CFBundleExecutable</key><string>MacSweep</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSAppleEventsUsageDescription</key><string>MacSweep asks Finder to move items to the Trash and quits apps before uninstalling them.</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
echo "Built $APP ($(du -sh "$APP" | cut -f1))"
