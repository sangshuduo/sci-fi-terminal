#!/usr/bin/env bash
# Assemble sci-fi-terminal.app from a release build (unsigned developer preview).
#
#   scripts/bundle-macos.sh [OUTPUT_DIR]      default: target/release/bundle
#
# Includes the .icns icon and, when fetched, the DB-IP database (found by the
# app in Contents/Resources). Run `cargo build -p sci-fi-terminal --release`
# and optionally `scripts/fetch-geoip.sh` first.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/target/release/bundle}"
binary="$root/target/release/sci-fi-terminal"
[[ -x "$binary" ]] || { echo "build first: cargo build -p sci-fi-terminal --release" >&2; exit 1; }
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -1)"

app="$out/sci-fi-terminal.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/sci-fi-terminal"
cp "$root/assets/icons/sci-fi-terminal.icns" "$app/Contents/Resources/"
sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
if [[ -f "$root/assets/geo/dbip-city-lite.mmdb" ]]; then
  cp "$root/assets/geo/dbip-city-lite.mmdb" "$app/Contents/Resources/"
fi
plutil -lint "$app/Contents/Info.plist" >/dev/null
echo "bundled: $app"
