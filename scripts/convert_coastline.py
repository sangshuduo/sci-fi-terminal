#!/usr/bin/env python3
"""Convert Natural Earth 1:110m coastline GeoJSON into the compact globe asset.

Source (public domain): https://github.com/nvkelso/natural-earth-vector
  geojson/ne_110m_coastline.geojson

Output format (little-endian):
  u16 polyline_count
  repeated: u16 point_count, then point_count * (i16 lon_centidegrees, i16 lat_centidegrees)

Usage: scripts/convert_coastline.py <input.geojson> <output.bin>
"""
import json
import struct
import sys


def main(src: str, dst: str) -> None:
    features = json.load(open(src, encoding="utf-8"))["features"]
    lines = []
    for feature in features:
        geometry = feature["geometry"]
        parts = [geometry["coordinates"]] if geometry["type"] == "LineString" else geometry["coordinates"]
        lines.extend(part for part in parts if len(part) >= 2)
    out = bytearray(struct.pack("<H", len(lines)))
    for line in lines:
        out += struct.pack("<H", len(line))
        for lon, lat in line:
            out += struct.pack("<hh", round(lon * 100), round(lat * 100))
    open(dst, "wb").write(out)
    print(f"{len(lines)} polylines, {sum(map(len, lines))} points, {len(out)} bytes")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
