#!/bin/bash
# Builds dist/MacSweep.app
#   ./bundle.sh              build for this Mac only
#   UNIVERSAL=1 ./bundle.sh  build one app for Apple Silicon + Intel
#   ZIP=1 ./bundle.sh        also create dist/MacSweep-<version>.zip for a release
set -euo pipefail
cd "$(dirname "$0")"

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
APP="dist/MacSweep.app"
rm -rf dist
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

if [[ "${UNIVERSAL:-0}" == "1" ]]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    lipo -create -output "$APP/Contents/MacOS/MacSweep" \
        target/aarch64-apple-darwin/release/macsweep \
        target/x86_64-apple-darwin/release/macsweep
else
    cargo build --release
    cp target/release/macsweep "$APP/Contents/MacOS/MacSweep"
fi

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
codesign --force --deep --sign "$SIGN_ID" "$APP"
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
