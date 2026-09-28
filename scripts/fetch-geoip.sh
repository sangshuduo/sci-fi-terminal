#!/usr/bin/env bash
# Download the pinned DB-IP City Lite database and verify it against
# assets/geo/dbip-city-lite.toml. Used by developers and by release CI.
#
#   scripts/fetch-geoip.sh            fetch (skips if a verified copy exists)
#   scripts/fetch-geoip.sh --update   fetch the manifest's month and print new hashes
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/assets/geo/dbip-city-lite.toml"

field() { sed -n "s/^$1 = \"\(.*\)\"$/\1/p" "$manifest"; }
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

month="$(field month)"
url="$(field url)"
gz_sha="$(field gz_sha256)"
mmdb_sha="$(field mmdb_sha256)"
out="$root/$(field output)"
update=false
[[ "${1:-}" == "--update" ]] && update=true
[[ "$url" == https://download.db-ip.com/free/* ]] || { echo "unexpected url: $url" >&2; exit 1; }

if [[ -f "$out" && "$update" == false && "$(sha256 "$out")" == "$mmdb_sha" ]]; then
  echo "verified: $out ($month)"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "downloading $url"
curl --fail --location --proto '=https' --tlsv1.2 --max-time 600 -o "$tmp/db.mmdb.gz" "$url"
got_gz="$(sha256 "$tmp/db.mmdb.gz")"
gunzip -c "$tmp/db.mmdb.gz" > "$tmp/db.mmdb"
got_mmdb="$(sha256 "$tmp/db.mmdb")"

if [[ "$update" == true ]]; then
  echo "month = \"$month\""
  echo "gz_sha256 = \"$got_gz\""
  echo "mmdb_sha256 = \"$got_mmdb\""
  echo "Paste these into $manifest after review."
  exit 0
fi

if [[ "$got_gz" != "$gz_sha" || "$got_mmdb" != "$mmdb_sha" ]]; then
  echo "checksum mismatch for $url" >&2
  echo "  expected gz $gz_sha, got $got_gz" >&2
  echo "  expected mmdb $mmdb_sha, got $got_mmdb" >&2
  exit 1
fi
mkdir -p "$(dirname "$out")"
mv "$tmp/db.mmdb" "$out.partial" && mv "$out.partial" "$out"
echo "verified: $out ($month)"
