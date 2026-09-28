#!/usr/bin/env bash
# Render the application icon from its SVG sources into every format we ship.
# Requires rsvg-convert and ImageMagick (magick); iconutil (macOS) for .icns.
#
#   assets/icons/sci-fi-terminal.svg        master artwork (≥ 64 px)
#   assets/icons/sci-fi-terminal-small.svg  simplified artwork (16–48 px)
#
# Outputs (committed, so builds need none of these tools):
#   assets/icons/png/icon-<N>.png           16 … 1024
#   assets/icons/sci-fi-terminal.ico        Windows (16–256)
#   assets/icons/sci-fi-terminal.icns       macOS (when iconutil is available)
#   assets/icons/window-icon-64.rgba        raw RGBA for the runtime window icon
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
icons="$root/assets/icons"
big="$icons/sci-fi-terminal.svg"
small="$icons/sci-fi-terminal-small.svg"
mkdir -p "$icons/png"

render() { # size -> png
  local size="$1" source="$big"
  [[ "$size" -le 48 ]] && source="$small"
  rsvg-convert -w "$size" -h "$size" "$source" -o "$icons/png/icon-$size.png"
}
for size in 16 32 48 64 128 256 512 1024; do render "$size"; done

magick "$icons"/png/icon-{16,32,48,64,128,256}.png "$icons/sci-fi-terminal.ico"
magick "$icons/png/icon-64.png" -depth 8 rgba:"$icons/window-icon-64.rgba"

if command -v iconutil >/dev/null 2>&1; then
  set_dir="$(mktemp -d)/sci-fi-terminal.iconset"
  mkdir -p "$set_dir"
  for size in 16 32 128 256 512; do
    cp "$icons/png/icon-$size.png" "$set_dir/icon_${size}x${size}.png"
    double=$((size * 2))
    cp "$icons/png/icon-$double.png" "$set_dir/icon_${size}x${size}@2x.png"
  done
  iconutil -c icns "$set_dir" -o "$icons/sci-fi-terminal.icns"
fi
echo "icons written to $icons"
