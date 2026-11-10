#!/usr/bin/env python3
"""Build one adversarial site on several hosts and compare the output file by file.

The same source must publish the same bytes wherever it is built. This script
writes a site that provokes every ordinary way a build can differ between
machines (equal dates, decomposed and composed Unicode file names, an event
whose past/upcoming state depends on the clock, Chinese text for search
segmentation, math, an image, title cards in Latin and Chinese, a place map
and a video), builds it with a given moss binary, and prints a sorted
`sha256  path` digest of everything the build published.

    cross-host-parity.py fixture <dir> [nfc|nfd]    write the site (any existing <dir> is refused)
    cross-host-parity.py build <moss> <dir> <out>   build <dir>, write its digest to <out>
    cross-host-parity.py compare <a> <b> ...        exit 1 and list the paths that differ

Run `build` under different TZ, LANG and RAYON_NUM_THREADS values, on each
host, cold (a fresh fixture) and warm (the same fixture again), and compare
every digest against the first. SOURCE_DATE_EPOCH is pinned so two runs on
different days still describe one source. `compare` sorts each difference
into a class (pages, title cards, map, video, ...), since outputs derived by an
outside renderer (ffmpeg, system fonts) are expected to differ between hosts
until the build reuses them instead of deriving them again.
"""

import hashlib
import json
import os
import struct
import subprocess
import sys
import unicodedata
import zlib
from pathlib import Path

# 2026-03-14 12:00 UTC. Every run of the gate builds "as of" this day.
PINNED_EPOCH = "1773489600"

# A short clip already in the repository, so every host transcodes the same
# input bytes.
CLIP = Path(__file__).resolve().parent.parent / "crates/moss-build/tests/fixtures/snapshot-sites/video-embed-site/input/clip.mp4"


def png(width: int, height: int, seed: int) -> bytes:
    """A small RGB PNG with a gradient, so image variants have real pixels to resample."""
    rows = b"".join(
        b"\x00" + bytes(((x * 7 + y * 3 + seed) % 256 for x in range(width) for _ in range(3)))
        for y in range(height)
    )

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def fixture(root: Path, form: str = "nfd") -> None:
    """`form` is how accented file names are written: the same logical site
    either way, as two sync clients may deliver it."""
    if root.exists():
        sys.exit(f"{root} exists; give a fresh directory")
    nfd = lambda s: unicodedata.normalize(form.upper(), s)
    files = {
        # Its own site, even when written inside another site's folder.
        # Places give the site a map: locator tiles beside a page and the
        # places root.
        ".moss/config.toml": '[terms.places]\ntype = "place"\nfields = ["location"]\ntitle = "Places"\n',
        ".moss/places.toml": '["Harbour"]\nlat = 35.0116\nlng = 135.7681\nprecision = "city"\n\n["Mill"]\nlat = 34.6851\nlng = 135.8048\nprecision = "exact"\n',
        "posts/walk.md": "---\ntitle: Walk\ndate: 2026-01-01\nlocation:\n  - Harbour\n  - Mill\n---\n\nFrom the harbour to the mill.\n",
        # Transcoded by ffmpeg into a poster, a thumbnail and an HLS ladder.
        "clip.mp4": CLIP.read_bytes(),
        "film.md": "---\ntitle: Film\n---\n\n![[clip.mp4]]\n",
        "index.md": "---\ntitle: Parity Hall\n---\n\nThe home page.\n",
        "posts/index.md": "---\ntitle: Posts\nchildren_style: list\n---\n",
        # Three articles on one date: their order in the feed and the listing
        # must not depend on the order the disk lists them.
        "posts/zeta.md": "---\ntitle: Zeta\ndate: 2026-01-05\n---\n\nZeta body.\n",
        "posts/alpha.md": "---\ntitle: Alpha\ndate: 2026-01-05\n---\n\nAlpha body.\n",
        "posts/mid.md": "---\ntitle: Mid\ndate: 2026-01-05\n---\n\nMid body.\n",
        # A page whose file name is decomposed on disk (as a Mac writes it on
        # some volumes and a Linux sync client keeps it), and one composed.
        nfd("posts/café.md"): "---\ntitle: Café\ndate: 2026-01-04\n---\n\nAn accented name.\n",
        "posts/naïve.md": "---\ntitle: Naive\ndate: 2026-01-03\n---\n\n![A gradient](../images/" + "crème.png" + ")\n",
        nfd("images/crème.png"): png(64, 48, 1),
        "images/plain.png": png(80, 40, 2),
        "posts/with-image.md": "---\ntitle: With image\ndate: 2026-01-02\n---\n\n![Plain](../images/plain.png)\n",
        # Clock-dependent: this event ends during the pinned day, so a build
        # that reads the wall clock in its own zone disagrees with one in
        # another zone about whether it is still upcoming.
        "events/index.md": "---\ntitle: Events\nchildren_group: upcoming\n---\n",
        "events/today.md": "---\ntitle: Same-day talk\nstart: 2026-03-14 09:00\nend: 2026-03-14 18:00\n---\n\nA talk.\n",
        "events/later.md": "---\ntitle: Later talk\nstart: 2099-01-01 10:00\n---\n\nLater.\n",
        "events/earlier.md": "---\ntitle: Earlier talk\nstart: 2001-01-01 10:00\n---\n\nEarlier.\n",
        "zh/index.md": "---\ntitle: 中文\nlang: zh-hans\n---\n\n今天我们在城市图书馆讨论开放出版和个人网站的未来。\n",
        "math.md": "---\ntitle: Math\n---\n\n$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$\n",
    }
    for rel, body in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(body if isinstance(body, bytes) else body.encode())


def published_dir(site: Path) -> Path:
    current = site / ".moss" / "build.nosync" / "current"
    if not (current / "index.html").exists():
        sys.exit(f"no built site at {current}")
    return current.resolve()


# The parts of the sealed manifest that describe the published site. The rest
# (render id, capture time, source stats) differ between any two runs.
MANIFEST_KEYS = ("files", "source_to_output", "page_meta")


def digest(out_dir: Path, manifest: Path) -> list[str]:
    sealed = json.loads(manifest.read_text())
    canonical = json.dumps({k: sealed.get(k) for k in MANIFEST_KEYS}, sort_keys=True, ensure_ascii=False)
    lines = [f"{hashlib.sha256(canonical.encode()).hexdigest()}  <manifest:{'+'.join(MANIFEST_KEYS)}>"]
    for path in sorted(out_dir.rglob("*")):
        if path.is_file():
            rel = unicodedata.normalize("NFC", path.relative_to(out_dir).as_posix())
            lines.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {rel}")
    return sorted(lines, key=lambda line: line.split("  ", 1)[1])


def build(moss: str, site: Path, out: Path) -> None:
    env = dict(os.environ, SOURCE_DATE_EPOCH=PINNED_EPOCH, HOME=os.environ.get("PARITY_HOME", str(site.parent / "home")))
    Path(env["HOME"]).mkdir(parents=True, exist_ok=True)
    subprocess.run([moss, "build", str(site)], env=env, check=True)
    out.write_text("\n".join(digest(published_dir(site), site / ".moss" / "build.nosync" / "hashes.json")) + "\n")


def kind(path: str) -> str:
    """The class of an output path, for reading a difference."""
    if path.startswith("<manifest"):
        return "manifest"
    if path.startswith("_moss/og/"):
        return "title card"
    if path.startswith("_moss/map") or path.startswith("_moss/places"):
        return "map"
    if ".hls/" in path or path.endswith((".poster.jpg", ".thumb.jpg", ".mp4")):
        return "video"
    if path.startswith("_moss/math"):
        return "math"
    if path.startswith("pagefind/") or "/pagefind/" in path:
        return "search"
    if path.endswith(".html"):
        return "page"
    if path.endswith((".xml", ".ics", ".json", ".txt")):
        return "feed or data"
    if path.startswith("images/") or path.endswith((".png", ".jpg", ".webp", ".avif")):
        return "image"
    return "other"


def compare(paths: list[Path]) -> int:
    def load(p: Path) -> dict[str, str]:
        return {line.split("  ", 1)[1]: line.split("  ", 1)[0] for line in p.read_text().splitlines() if line}

    first, status = load(paths[0]), 0
    for other_path in paths[1:]:
        other = load(other_path)
        differ = sorted(k for k in first.keys() | other.keys() if first.get(k) != other.get(k))
        if differ:
            status = 1
            by_kind: dict[str, list[str]] = {}
            for k in differ:
                by_kind.setdefault(kind(k), []).append(k)
            print(f"{paths[0]} vs {other_path}: {len(differ)} path(s) differ")
            for name, keys in sorted(by_kind.items()):
                print(f"  {name}: {len(keys)}")
                for k in keys:
                    print(f"    {k}  {first.get(k, '-')[:12]} {other.get(k, '-')[:12]}")
    return status


if __name__ == "__main__":
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "fixture":
        fixture(Path(args[0]), *args[1:])
    elif cmd == "build":
        build(args[0], Path(args[1]), Path(args[2]))
    elif cmd == "compare":
        sys.exit(compare([Path(a) for a in args]))
    else:
        sys.exit(__doc__)
