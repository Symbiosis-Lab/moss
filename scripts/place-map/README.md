# Place-map source boundary

`generate.py` is the offline boundary for the future place-map geometry generator. It does not fetch sources, decode shapefiles, or produce a map pack. It verifies the exact source manifest, archive bytes, required shapefile sidecars, and GMT server manifest before a later generator is allowed to consume them.

Put the pinned source objects and `earth_relief_server.txt` in a separate cache, then run `MOSS_PLACE_MAP_SOURCE_DIR=/path/to/cache python3 generate.py --check`. The cache is intentionally outside the repository; a missing object or hash mismatch is an error and never triggers a network fallback. `--generate OUTPUT` writes a deterministic verified-source receipt for the next generator stage.

The source manifest records the Natural Earth layer release metadata, exact ZIP members, SHA-256 values, and public-domain notice. The GMT server manifest is pinned by date and SHA-256; because that upstream manifest does not publish a separate licence statement, `NOTICE` preserves that limitation instead of asserting one.
