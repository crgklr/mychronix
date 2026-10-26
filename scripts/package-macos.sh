#!/bin/bash
# Builds the Mac download into target/dist: Mychronix-<version>-arm64.dmg,
# holding Mychronix.app (with the `mychronix` command-line tool inside it at
# Mychronix.app/Contents/MacOS/mychronix) and a shortcut to Applications.
#
# Runs on an Apple Silicon Mac (the release workflow uses GitHub's). Needs
# rsvg-convert (brew install librsvg) for the icon. The app is signed ad hoc,
# not with an Apple Developer ID, so the first launch needs
# System Settings → Privacy & Security → Open Anyway.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
dist=target/dist
app=target/macos/Mychronix.app
rm -rf target/macos
mkdir -p "$dist" "$app/Contents/MacOS" "$app/Contents/Resources"

export MACOSX_DEPLOYMENT_TARGET=11.0
cargo build --release --locked -p mychronix-gui -p mychronix-cli

cp target/release/mychronix-gui target/release/mychronix "$app/Contents/MacOS/"

# Icon: every size macOS asks for, from the SVG.
iconset=target/macos/Mychronix.iconset
mkdir -p "$iconset"
svg=crates/mychronix-gui/assets/mychronix-icon.svg
for size in 16 32 128 256 512; do
  rsvg-convert -w "$size" -h "$size" "$svg" -o "$iconset/icon_${size}x${size}.png"
  rsvg-convert -w $((size * 2)) -h $((size * 2)) "$svg" -o "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Mychronix.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Mychronix</string>
  <key>CFBundleDisplayName</key><string>Mychronix</string>
  <key>CFBundleIdentifier</key><string>io.github.crgklr.mychronix</string>
  <key>CFBundleExecutable</key><string>mychronix-gui</string>
  <key>CFBundleIconFile</key><string>Mychronix</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.sports</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>Mychronix talks to your MyChron over its WiFi network to download sessions.</string>
  <key>NSHumanReadableCopyright</key><string>© 2026 Mychronix contributors. Free and open source (MIT or Apache 2.0).</string>
</dict>
</plist>
PLIST

# The About window's text, under the name and version.
cat > "$app/Contents/Resources/Credits.html" <<'HTML'
<html><body style="font-family: -apple-system, sans-serif; font-size: 11px; text-align: center">
<p>Download and analyze sessions from AiM MyChron data loggers.</p>
<p><a href="https://github.com/crgklr/mychronix">github.com/crgklr/mychronix</a></p>
<p><i>Dedicated to my two boys</i></p>
<p style="color: gray">An independent project, not affiliated with AiM.</p>
</body></html>
HTML

# Ad-hoc signature: Apple Silicon won't run unsigned code, and without one a
# downloaded app shows as "damaged" instead of offering Open Anyway.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# The disk image: the app, a shortcut to drag it onto, and how to open it.
stage=target/macos/dmg
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
cat > "$stage/First launch.txt" <<'TXT'
Drag Mychronix into Applications, then open it.

Mychronix isn't signed with an Apple Developer ID, so the first time macOS
says it can't verify it. Click Done, then open System Settings → Privacy &
Security, scroll down and click "Open Anyway" next to Mychronix. You only
need to do this once per version.

To download from a MyChron: turn on its WiFi, choose its network (AiM-MYC…)
from the WiFi menu in the menu bar, and Mychronix finds it. If macOS asks
whether Mychronix may find devices on your local network, click Allow.
TXT
hdiutil create -volname "Mychronix" -srcfolder "$stage" -ov -format UDZO "$dist/Mychronix-$version-arm64.dmg"

(cd "$dist" && shasum -a 256 Mychronix-*.dmg > SHA256SUMS-macos)
ls -l "$dist"
