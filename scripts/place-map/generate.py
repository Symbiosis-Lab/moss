#!/usr/bin/env python3
"""Verify the checked-in place-map source boundary without network access.

This is deliberately not the geometry generator. It gives that generator one
closed, deterministic input contract: every source must be present in an
explicit cache, match the manifest hash and byte size, and contain the exact
shape members named by the manifest. ``--generate`` writes a canonical source
receipt that is useful as the next generator's input, but never writes a map
pack or fetches a missing source.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import tomllib
import zipfile


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
MANIFEST = REPO_ROOT / "crates/moss-build/data/place-map/source-manifest.toml"
GMT_MANIFEST_ARTIFACT = "earth_relief_server.txt"
EXPECTED_NAMES = {
    "coastline",
    "land",
    "lakes",
    "rivers",
    "glaciated_areas",
    "antarctic_ice_shelves",
    "reefs",
    "playas",
    "bathymetry",
    "urban_areas",
    "earth_relief_06m_g",
}
EXPECTED_URLS = {
    "coastline": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_coastline.zip",
    "land": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_land.zip",
    "lakes": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_lakes.zip",
    "rivers": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_rivers_lake_centerlines_scale_rank.zip",
    "glaciated_areas": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_glaciated_areas.zip",
    "antarctic_ice_shelves": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_antarctic_ice_shelves_polys.zip",
    "reefs": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_reefs.zip",
    "playas": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_playas.zip",
    "bathymetry": "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_bathymetry_all.zip",
    "urban_areas": "https://naturalearth.s3.amazonaws.com/10m_cultural/ne_10m_urban_areas.zip",
    "earth_relief_06m_g": "https://oceania.generic-mapping-tools.org/server/earth/earth_relief/earth_relief_06m_g.grd",
}
EXPECTED_PUBLISHED_VERSIONS = {
    "coastline": "4.1.0",
    "land": "5.1.1",
    "lakes": "5.0.0",
    "rivers": "5.0.0",
    "glaciated_areas": "4.1.0",
    "antarctic_ice_shelves": "4.1.0",
    "reefs": "4.1.0",
    "playas": "5.0.0",
    "bathymetry": "4.1.0",
    "urban_areas": "4.1.0",
}


class VerificationError(ValueError):
    """A source boundary violation that must stop generation."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_manifest() -> tuple[dict, str]:
    try:
        raw = MANIFEST.read_bytes()
        return tomllib.loads(raw.decode("utf-8")), hashlib.sha256(raw).hexdigest()
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise VerificationError(f"cannot read {MANIFEST}: {error}") from error


def validate_manifest(manifest: dict) -> list[dict]:
    if manifest.get("schema") != 1:
        raise VerificationError("unsupported place-map manifest schema")
    if manifest.get("natural_earth_release") != "mixed per-layer releases; see each source's published_version and archive_version":
        raise VerificationError("Natural Earth releases must be stated per source")
    if "natural_earth_version" in manifest:
        raise VerificationError("a single Natural Earth version cannot describe these mixed archives")
    if manifest.get("gmt_dataset") != "earth_relief_06m_g":
        raise VerificationError("only GMT earth_relief_06m_g is allowed")
    if manifest.get("gmt_manifest_date") != "2025-05-01":
        raise VerificationError("GMT manifest date is not the pinned 2025-05-01")
    if manifest.get("natural_earth_license", "").lower().find("public domain") < 0:
        raise VerificationError("Natural Earth public-domain notice is missing")
    if "border" in json.dumps(manifest, sort_keys=True).lower():
        raise VerificationError("administrative-border sources are forbidden")

    sources = manifest.get("sources")
    names = [source.get("name") for source in sources] if isinstance(sources, list) else []
    if len(names) != len(set(names)):
        raise VerificationError("manifest contains duplicate source names")
    if not isinstance(sources, list) or set(names) != EXPECTED_NAMES:
        raise VerificationError("manifest source set differs from the pinned source set")
    for source in sources:
        name = source.get("name")
        if source.get("url") != EXPECTED_URLS[name]:
            raise VerificationError(f"{name}: URL is not the pinned source")
        if name != "earth_relief_06m_g" and source.get("published_version") != EXPECTED_PUBLISHED_VERSIONS[name]:
            raise VerificationError(f"{name}: published version is not the pinned layer version")
        artifact = source.get("artifact")
        if not isinstance(artifact, str) or Path(artifact).name != artifact:
            raise VerificationError(f"{name}: artifact must be a plain filename")
        digest = source.get("sha256", "")
        if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
            raise VerificationError(f"{name}: invalid lowercase SHA-256")
        size = source.get("size_bytes")
        if not isinstance(size, int) or size <= 0:
            raise VerificationError(f"{name}: positive size_bytes is required")
        members = source.get("members")
        member = source.get("member")
        if members is not None and (member is not None or not members or any("*" in value for value in members)):
            raise VerificationError(f"{name}: members must be an exact list, not a wildcard")
        if members is None and (not isinstance(member, str) or "*" in member):
            raise VerificationError(f"{name}: one exact member is required")
    gmt_hash = manifest.get("gmt_manifest_sha256", "")
    if len(gmt_hash) != 64 or any(char not in "0123456789abcdef" for char in gmt_hash):
        raise VerificationError("GMT server manifest hash is required")
    if manifest.get("gmt_license") != "not specified in the pinned GMT server manifest":
        raise VerificationError("GMT licence status must remain explicit")
    return sources


def verify_source(source: dict, source_dir: Path) -> dict:
    path = source_dir / source["artifact"]
    source_root = source_dir.resolve()
    if path.is_symlink() or path.resolve().parent != source_root:
        raise VerificationError(f"{source['name']}: artifact must be a regular file directly in the source cache")
    if not path.is_file():
        raise VerificationError(f"missing cached source: {path}")
    size = path.stat().st_size
    if size != source["size_bytes"]:
        raise VerificationError(f"{source['name']}: size {size} != {source['size_bytes']}")
    actual = sha256(path)
    if actual != source["sha256"]:
        raise VerificationError(f"{source['name']}: SHA-256 {actual} != {source['sha256']}")
    expected_members = source.get("members") or [source["member"]]
    members: list[str] = []
    if path.suffix == ".zip":
        try:
            with zipfile.ZipFile(path) as archive:
                members = archive.namelist()
        except (OSError, zipfile.BadZipFile) as error:
            raise VerificationError(f"{source['name']}: invalid ZIP: {error}") from error
        missing = sorted(set(expected_members) - set(members))
        if missing:
            raise VerificationError(f"{source['name']}: missing ZIP members {missing}")
        version_member = source.get("version_member", f"{Path(source['artifact']).stem}.VERSION.txt")
        if version_member not in members:
            raise VerificationError(f"{source['name']}: missing version member {version_member}")
        with zipfile.ZipFile(path) as archive:
            archive_version = archive.read(version_member).decode("ascii").strip().splitlines()[0]
        expected_version = source.get("archive_version", source.get("published_version"))
        if archive_version != expected_version:
            raise VerificationError(f"{source['name']}: archive VERSION {archive_version} != {expected_version}")
    elif expected_members != [path.name]:
        raise VerificationError(f"{source['name']}: non-ZIP member must name the artifact")
    return {
        "name": source["name"],
        "artifact": source["artifact"],
        "sha256": actual,
        "size_bytes": size,
        "members": expected_members,
        "layer": source["layer"],
    }


def write_receipt(output: Path, receipt: dict) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n", encoding="utf-8")


def verify(source_dir: Path) -> dict:
    manifest, manifest_sha256 = load_manifest()
    sources = validate_manifest(manifest)
    receipts = [verify_source(source, source_dir) for source in sorted(sources, key=lambda item: item["name"])]
    gmt_manifest = source_dir / GMT_MANIFEST_ARTIFACT
    if gmt_manifest.is_symlink() or gmt_manifest.resolve().parent != source_dir.resolve():
        raise VerificationError("GMT server manifest must be a regular file directly in the source cache")
    if not gmt_manifest.is_file():
        raise VerificationError(f"missing cached GMT server manifest: {gmt_manifest}")
    gmt_hash = sha256(gmt_manifest)
    if gmt_hash != manifest["gmt_manifest_sha256"]:
        raise VerificationError(f"GMT server manifest SHA-256 {gmt_hash} != {manifest['gmt_manifest_sha256']}")
    return {
        "schema": 1,
        "generator": manifest["generator"],
        "generator_version": manifest["generator_version"],
        "source_manifest_sha256": manifest_sha256,
        "gmt_manifest_sha256": gmt_hash,
        "sources": receipts,
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=None, help="offline cache containing the pinned source objects")
    parser.add_argument("--check", action="store_true", help="verify the manifest and every cached source")
    parser.add_argument("--generate", type=Path, metavar="OUTPUT", help="write a deterministic verified-source receipt")
    args = parser.parse_args(argv)
    source_dir = args.source_dir or (Path(os.environ["MOSS_PLACE_MAP_SOURCE_DIR"]) if os.environ.get("MOSS_PLACE_MAP_SOURCE_DIR") else None)
    if source_dir is None:
        parser.error("--source-dir or MOSS_PLACE_MAP_SOURCE_DIR is required; network fallback is disabled")
    if not args.check and args.generate is None:
        parser.error("choose --check or --generate OUTPUT")
    try:
        receipt = verify(source_dir.resolve())
        if args.generate is not None:
            write_receipt(args.generate, receipt)
        print(f"verified {len(receipt['sources'])} sources; manifest {receipt['source_manifest_sha256']}")
        return 0
    except (OSError, VerificationError) as error:
        print(f"place-map source verification failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
