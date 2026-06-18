#!/usr/bin/env bash
# macOS only: wraps the binary into a proper .app bundle
set -e

APP="QR Frame.app"

mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"

cp target/release/qr-frame "$APP/Contents/MacOS/qr-frame"

# Bundle icon (optional but recommended)
if [ -f assets/icon.icns ]; then
    cp assets/icon.icns "$APP/Contents/Resources/icon.icns"
fi

PLIST="$APP/Contents/Info.plist"

printf '<?xml version="1.0" encoding="UTF-8"?>\n' > "$PLIST"
printf '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n' >> "$PLIST"
printf '<plist version="1.0"><dict>\n' >> "$PLIST"
printf '  <key>CFBundleExecutable</key><string>qr-frame</string>\n' >> "$PLIST"
printf '  <key>CFBundleIdentifier</key><string>com.berkay.qr-frame</string>\n' >> "$PLIST"
printf '  <key>CFBundleName</key><string>QR Frame</string>\n' >> "$PLIST"
printf '  <key>CFBundleVersion</key><string>1.0</string>\n' >> "$PLIST"
printf '  <key>CFBundleIconFile</key><string>icon.icns</string>\n' >> "$PLIST"
printf '  <key>NSHighResolutionCapable</key><true/>\n' >> "$PLIST"
printf '  <key>NSScreenCaptureUsageDescription</key>\n' >> "$PLIST"
printf '  <string>QR Frame needs screen recording to read QR codes.</string>\n' >> "$PLIST"
printf '</dict></plist>\n' >> "$PLIST"

echo "  [+] Created: $APP"
echo "      Drag to /Applications to install."
