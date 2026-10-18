#!/bin/bash
# Builds Mychronix's Linux downloads into target/dist:
#   mychronix-<version>-<arch>.tar.gz    the two programs, launcher and icons
#   mychronix_<version>-1_<arch>.deb      Debian, Ubuntu, Mint, Pop!_OS
#   mychronix-<version>-1.<arch>.rpm     Fedora, openSUSE
#   Mychronix-<version>-<arch>.AppImage  any distro, no install (and its
#                                        .zsync, for AppImage updaters)
# Needs cargo-deb and cargo-generate-rpm (cargo install them), rsvg-convert
# (librsvg) or ImageMagick for the icons, and downloads appimagetool.
# The release workflow runs this; it also works locally.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
dist=target/dist
rm -rf "$dist" target/AppDir
mkdir -p "$dist"

cargo build --release --locked -p mychronix-gui -p mychronix-cli

# Icons from the SVG, at the sizes desktops look for.
svg=crates/mychronix-gui/assets/mychronix-icon.svg
for size in 48 128 256 512; do
  mkdir -p "target/icons/${size}x${size}"
  png="target/icons/${size}x${size}/mychronix.png"
  if command -v rsvg-convert >/dev/null; then
    rsvg-convert -w "$size" -h "$size" "$svg" -o "$png"
  else
    magick -background none -density 600 "$svg" -resize "${size}x${size}" "$png"
  fi
done

# Tarball: unpack anywhere; bin/, share/ follow the usual layout.
stage="target/mychronix-$version-$arch"
rm -rf "$stage"
install -Dm755 target/release/mychronix-gui "$stage/bin/mychronix-gui"
install -Dm755 target/release/mychronix "$stage/bin/mychronix"
install -Dm644 packaging/linux/70-mychronix-aim.rules "$stage/share/mychronix/70-mychronix-aim.rules"
install -Dm644 packaging/linux/mychronix.desktop "$stage/share/applications/mychronix.desktop"
install -Dm644 "$svg" "$stage/share/icons/hicolor/scalable/apps/mychronix.svg"
for size in 48 128 256 512; do
  install -Dm644 "target/icons/${size}x${size}/mychronix.png" "$stage/share/icons/hicolor/${size}x${size}/apps/mychronix.png"
done
install -Dm644 README.md LICENSE-MIT LICENSE-APACHE -t "$stage/"
tar -C target -czf "$dist/mychronix-$version-$arch.tar.gz" "mychronix-$version-$arch"

# .deb and .rpm, from the metadata in crates/mychronix-gui/Cargo.toml.
cargo deb -p mychronix-gui --no-build --output "$dist/"
cargo generate-rpm -p crates/mychronix-gui --output "$dist/"

# AppImage: the tarball's layout plus the launcher and icon at the top.
appdir=target/AppDir
mkdir -p "$appdir/usr"
cp -r "$stage/bin" "$stage/share" "$appdir/usr/"
cp packaging/linux/mychronix.desktop "$appdir/"
cp "target/icons/256x256/mychronix.png" "$appdir/mychronix.png"
ln -sf usr/bin/mychronix-gui "$appdir/AppRun"
tool=target/appimagetool-$arch.AppImage
if [ ! -x "$tool" ]; then
  curl -fsSL -o "$tool" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage"
  chmod +x "$tool"
fi
# Update info, so AppImage updaters (AppImageUpdate, Gear Lever and the like)
# fetch new versions from GitHub's releases; appimagetool writes the .zsync
# file they compare against beside the AppImage.
repo=${GITHUB_REPOSITORY:-crgklr/mychronix}
update="gh-releases-zsync|${repo%%/*}|${repo#*/}|latest|Mychronix-*-$arch.AppImage.zsync"
# Runs without FUSE (CI machines don't have it).
(cd "$dist" && APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$arch" "$OLDPWD/$tool" --no-appstream -u "$update" "$OLDPWD/$appdir" "Mychronix-$version-$arch.AppImage")

(cd "$dist" && sha256sum -- * > SHA256SUMS)
ls -l "$dist"
