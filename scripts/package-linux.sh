#!/bin/bash
# Build JInk for Linux x86_64 and package it as .deb and .AppImage in target/release/.
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
OUT="$DIR/target/release"
BIN="$OUT/jink"

[ "${SKIP_BUILD:-0}" = "1" ] || cargo build --release

DESKTOP='[Desktop Entry]
Type=Application
Name=JInk
Comment=Quick input popup: type, AI-enhance, translate, voice
Exec=jink
Icon=jink
Terminal=false
Categories=Utility;'

# ---- .deb
PKG="$(mktemp -d)/jink_${VERSION}_amd64"
install -Dm755 "$BIN" "$PKG/usr/bin/jink"
install -Dm644 assets/icons/icon.png "$PKG/usr/share/icons/hicolor/512x512/apps/jink.png"
install -Dm644 assets/icons/128x128.png "$PKG/usr/share/icons/hicolor/128x128/apps/jink.png"
mkdir -p "$PKG/usr/share/applications" "$PKG/DEBIAN"
echo "$DESKTOP" > "$PKG/usr/share/applications/jink.desktop"
cat > "$PKG/DEBIAN/control" <<EOF
Package: jink
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: nhatcoi
Depends: libgtk-3-0, libxkbcommon-x11-0, libvulkan1, libasound2, xdotool, wmctrl, x11-utils
Recommends: libayatana-appindicator3-1 | libappindicator3-1, espeak-ng
Description: Quick input popup with AI enhance, translate and voice
 Global hotkey opens a popup; the text is injected into the focused app.
EOF
dpkg-deb --root-owner-group --build "$PKG" "$OUT/JInk-$VERSION-linux-amd64.deb"

# ---- .AppImage (system libs like GTK/Vulkan come from the host)
APPDIR="$(mktemp -d)/JInk.AppDir"
install -Dm755 "$BIN" "$APPDIR/usr/bin/jink"
cp assets/icons/icon.png "$APPDIR/jink.png"
echo "$DESKTOP" > "$APPDIR/jink.desktop"
ln -s usr/bin/jink "$APPDIR/AppRun"

TOOL="${APPIMAGETOOL:-$OUT/appimagetool}"
if [ ! -x "$TOOL" ]; then
    curl -fsSL -o "$TOOL" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$TOOL"
fi
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOL" "$APPDIR" "$OUT/JInk-$VERSION-linux-x86_64.AppImage"

echo "==> $OUT/JInk-$VERSION-linux-amd64.deb"
echo "==> $OUT/JInk-$VERSION-linux-x86_64.AppImage"
