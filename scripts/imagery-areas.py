#!/usr/bin/env python3
"""Writes crates/mychronix/assets/imagery-areas.json: the outline of each area
whose national aerial photos Mychronix uses (crates/mychronix/src/imagery.rs),
from Natural Earth's public-domain 1:10m borders.

    scripts/imagery-areas.py CH NL ES FR DE-NW BE-VLG=BE-VAN+BE-VBR ...

Country codes are ISO 3166-1 (CH), state and province codes ISO 3166-2
(DE-NW); NAME=CODE+CODE+... joins several into one area (Flanders from its
provinces). Outlines are simplified to about 200 m: a map tile is only drawn
from a national source when it lies wholly inside the area, so a coarse
border costs nothing but a little imagery near it.
"""

import json
import math
import sys
import urllib.request
from pathlib import Path

BASE = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson/"
COUNTRIES = "ne_10m_admin_0_countries.geojson"
STATES = "ne_10m_admin_1_states_provinces.geojson"
TOLERANCE_DEG = 0.002  # about 200 m
OUT = Path(__file__).resolve().parent.parent / "crates/mychronix/assets/imagery-areas.json"


def load(name, cache):
    path = cache / name
    if not path.exists():
        print(f"downloading {name}", file=sys.stderr)
        req = urllib.request.Request(BASE + name, headers={"User-Agent": "Mychronix build script"})
        with urllib.request.urlopen(req, timeout=300) as r:
            path.write_bytes(r.read())
    return json.loads(path.read_text(encoding="utf-8"))


def simplify(points, tol):
    """Douglas-Peucker, iterative."""
    if len(points) < 4:
        return points
    keep = [False] * len(points)
    keep[0] = keep[-1] = True
    stack = [(0, len(points) - 1)]
    while stack:
        a, b = stack.pop()
        (ax, ay), (bx, by) = points[a], points[b]
        dx, dy = bx - ax, by - ay
        norm = math.hypot(dx, dy)
        far, far_d = None, tol
        for i in range(a + 1, b):
            px, py = points[i]
            d = abs(dy * px - dx * py + bx * ay - by * ax) / norm if norm else math.hypot(px - ax, py - ay)
            if d > far_d:
                far, far_d = i, d
        if far is not None:
            keep[far] = True
            stack += [(a, far), (far, b)]
    return [p for p, k in zip(points, keep) if k]


def rings(geometry):
    """Outer rings only: holes (lakes, enclaves) are left in, they're rare and small."""
    polys = geometry["coordinates"] if geometry["type"] == "MultiPolygon" else [geometry["coordinates"]]
    for poly in polys:
        ring = simplify([(round(x, 4), round(y, 4)) for x, y in poly[0]], TOLERANCE_DEG)
        if len(ring) >= 4:
            yield [[x, y] for x, y in ring]


def main(args):
    # NAME=A+B joins A and B as NAME; a plain code is its own area.
    parts = {}
    for arg in args:
        name, _, joined = arg.partition("=")
        for code in (joined or name).split("+"):
            parts[code] = name
    codes = list(parts)
    cache = Path(sys.argv[0]).resolve().parent.parent / "target" / "natural-earth"
    cache.mkdir(parents=True, exist_ok=True)
    found = {}
    if any("-" not in c for c in codes):
        for f in load(COUNTRIES, cache)["features"]:
            p = f["properties"]
            # ISO_A2 is "-99" for a few (France, Norway); ISO_A2_EH fills those in.
            code = p.get("ISO_A2_EH") if p.get("ISO_A2") in (None, "-99") else p.get("ISO_A2")
            if code in codes:
                found.setdefault(code, []).extend(rings(f["geometry"]))
    if any("-" in c for c in codes):
        for f in load(STATES, cache)["features"]:
            code = f["properties"].get("iso_3166_2")
            if code in codes:
                found.setdefault(code, []).extend(rings(f["geometry"]))
    missing = [c for c in codes if c not in found]
    if missing:
        sys.exit(f"not in Natural Earth: {' '.join(missing)}")
    areas = {}
    for code, rs in found.items():
        areas.setdefault(parts[code], []).extend(rs)
    found = areas
    OUT.write_text(json.dumps({c: found[c] for c in sorted(found)}, separators=(",", ":")) + "\n")
    points = sum(len(r) for rs in found.values() for r in rs)
    print(f"{OUT.name}: {len(found)} areas, {points} points, {OUT.stat().st_size // 1024} KB", file=sys.stderr)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
