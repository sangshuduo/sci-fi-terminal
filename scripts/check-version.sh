#!/usr/bin/env bash
# Print the release version from VERSION after checking that it is plain
# semver (MAJOR.MINOR.PATCH) and matches the Cargo workspace version.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(tr -d '[:space:]' < "$root/VERSION")"
if ! [[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "VERSION must be MAJOR.MINOR.PATCH, got '$version'" >&2
  exit 1
fi
cargo_version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml")"
if [[ "$version" != "$cargo_version" ]]; then
  echo "VERSION ($version) does not match [workspace.package] version ($cargo_version) in Cargo.toml" >&2
  exit 1
fi
echo "$version"
