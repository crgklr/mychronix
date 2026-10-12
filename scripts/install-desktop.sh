#!/bin/bash
# Builds Mychronix and installs it for the current user: the app in
# ~/.local/bin, its icon in the hicolor theme, and a launcher so it appears in
# the app drawer and Alt+Tab with the MyChron icon.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release -p mychronix-gui -p mychronix-cli
install -Dm755 target/release/mychronix-gui "$HOME/.local/bin/mychronix-gui"
install -Dm755 target/release/mychronix "$HOME/.local/bin/mychronix"

icons="$HOME/.local/share/icons/hicolor"
svg=crates/mychronix-gui/assets/mychronix-icon.svg
install -Dm644 "$svg" "$icons/scalable/apps/mychronix.svg"
for size in 16 24 32 48 64 128 256 512; do
  mkdir -p "$icons/${size}x${size}/apps"
  magick -background none -density 600 "$svg" -resize "${size}x${size}" "$icons/${size}x${size}/apps/mychronix.png"
done

# Point the launcher at the installed binary by full path, so it doesn't depend on PATH.
install -d "$HOME/.local/share/applications"
sed "s|^Exec=mychronix-gui|Exec=$HOME/.local/bin/mychronix-gui|" packaging/linux/mychronix.desktop \
  > "$HOME/.local/share/applications/mychronix.desktop"
gtk-update-icon-cache -q -t -f "$icons" 2>/dev/null || true
update-desktop-database -q "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed. Mychronix is in your app drawer; ~/.local/bin/mychronix-gui runs it from a terminal."
