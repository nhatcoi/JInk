#!/bin/bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

echo "==> Building JInk in release mode..."
cargo build --release

APP_NAME="JInk"
APP_DIR="$DIR/target/release/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

echo "==> Creating macOS App Bundle at $APP_DIR..."
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR"
mkdir -p "$RESOURCES_DIR"

# Copy binary
cp "$DIR/target/release/jink" "$MACOS_DIR/jink"
chmod +x "$MACOS_DIR/jink"

# Copy icon
if [ -f "$DIR/assets/icons/icon.icns" ]; then
    cp "$DIR/assets/icons/icon.icns" "$RESOURCES_DIR/icon.icns"
fi

# Create Info.plist
cat <<EOF > "$CONTENTS_DIR/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>jink</string>
    <key>CFBundleIdentifier</key>
    <string>com.nhatcoi.jink</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.2.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleIconFile</key>
    <string>icon.icns</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>JInk listens to your voice to turn speech into text.</string>
    <key>NSSpeechRecognitionUsageDescription</key>
    <string>JInk turns what you say into text with the built-in speech recognizer.</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

# Sign with a real identity so macOS privacy grants (Accessibility, Microphone, Speech)
# survive rebuilds. Ad-hoc signatures are pinned to the binary hash, so every build
# would look like a new app and silently lose Accessibility (no selection, no paste).
# Override with JINK_SIGN_ID="<identity>"; set JINK_SIGN_ID=- to force ad-hoc.
SIGN_ID="${JINK_SIGN_ID:-$(security find-identity -v -p codesigning 2>/dev/null | awk '/Apple Development|Developer ID Application/ {print $2; exit}')}"
if [ -n "$SIGN_ID" ]; then
    echo "==> Signing with $SIGN_ID..."
    codesign --force --deep --sign "$SIGN_ID" --identifier com.nhatcoi.jink "$APP_DIR"
else
    echo "==> No signing identity found: ad-hoc signed, privacy grants reset on every rebuild."
    codesign --force --deep --sign - --identifier com.nhatcoi.jink "$APP_DIR"
fi

echo "==> Successfully created $APP_DIR!"
echo "You can launch it directly with: open $APP_DIR"

