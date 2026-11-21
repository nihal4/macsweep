#!/bin/bash
# Builds dist/MacSweep.app
#   ./bundle.sh              build for this Mac only
#   UNIVERSAL=1 ./bundle.sh  build one app for Apple Silicon + Intel
#   ZIP=1 ./bundle.sh        also create dist/MacSweep-<version>.zip for a release
set -euo pipefail
cd "$(dirname "$0")"

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
APP="dist/MacSweep.app"
# The menu bar monitor is a separate tiny app inside MacSweep.app, started as a
# login item only if the user turns it on.
HELPER="$APP/Contents/Library/LoginItems/MacSweep Monitor.app"
rm -rf dist
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$HELPER/Contents/MacOS" "$HELPER/Contents/Resources"

if [[ "${UNIVERSAL:-0}" == "1" ]]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    cargo build --release --workspace --target aarch64-apple-darwin
    cargo build --release --workspace --target x86_64-apple-darwin
    lipo -create -output "$APP/Contents/MacOS/MacSweep" \
        target/aarch64-apple-darwin/release/macsweep \
        target/x86_64-apple-darwin/release/macsweep
    lipo -create -output "$HELPER/Contents/MacOS/macsweep-monitor" \
        target/aarch64-apple-darwin/release/macsweep-monitor \
        target/x86_64-apple-darwin/release/macsweep-monitor
else
    cargo build --release --workspace
    cp target/release/macsweep "$APP/Contents/MacOS/MacSweep"
    cp target/release/macsweep-monitor "$HELPER/Contents/MacOS/macsweep-monitor"
fi

cp assets/AppIcon.icns "$HELPER/Contents/Resources/AppIcon.icns"
cat > "$HELPER/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>MacSweep Monitor</string>
    <key>CFBundleDisplayName</key><string>MacSweep Monitor</string>
    <key>CFBundleIdentifier</key><string>com.nihal.macsweep.monitor</string>
    <key>CFBundleExecutable</key><string>macsweep-monitor</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>MacSweep</string>
    <key>CFBundleDisplayName</key><string>MacSweep</string>
    <key>CFBundleIdentifier</key><string>com.nihal.macsweep</string>
    <key>CFBundleExecutable</key><string>MacSweep</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSAppleEventsUsageDescription</key><string>MacSweep asks Finder to move items to the Trash and quits apps before uninstalling them.</string>
</dict>
</plist>
PLIST

# Sign. With a stable local certificate (scripts/setup-signing.sh) macOS keeps
# Full Disk Access across rebuilds; otherwise fall back to an ad-hoc signature,
# which macOS treats as a brand-new app after every build.
SIGN_ID="-"
if [[ "${ADHOC:-0}" != "1" ]] && security find-identity -p codesigning 2>/dev/null | grep -q "MacSweep Local Signing"; then
    SIGN_ID="MacSweep Local Signing"
fi
# Inner app first, then the outer one that seals it.
codesign --force --sign "$SIGN_ID" "$HELPER"
codesign --force --sign "$SIGN_ID" "$APP"
if [[ "$SIGN_ID" == "-" ]]; then
    echo "Note: ad-hoc signed. After installing this build, re-add MacSweep in Full Disk Access"
    echo "      (or run scripts/setup-signing.sh once so you never have to again)."
else
    echo "Signed with '$SIGN_ID' — Full Disk Access carries over."
fi
echo "Built $APP v${VERSION} ($(du -sh "$APP" | cut -f1))"

if [[ "${ZIP:-0}" == "1" ]]; then
    (cd dist && ditto -c -k --keepParent MacSweep.app "MacSweep-${VERSION}.zip")
    echo "Packed dist/MacSweep-${VERSION}.zip"
fi
