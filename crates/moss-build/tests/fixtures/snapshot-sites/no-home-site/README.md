# no-home-site

Covers: builder edge case — no corresponding doc page

A site with content but **no home file** at the root: no `index.md`, no `readme.md`, no folder note. `html.rs` synthesizes the homepage instead, and until this fixture existed that arm was uncovered — `empty-site` is the only other fixture without a root home file and it contains no markdown at all, so nothing exercised the synthesized listing.

Also the regression fixture for asset folders being published as sections. `[editor].attachment_folder = "./assets"` makes every `assets/` directory here storage, so the build must emit no `index.html` under any of them — while still emitting each image's variants, which is what proves the exclusion narrowed the index-page seed and not the scan walk. Before the fix this site shipped four asset listing pages, and the synthesized homepage listed them as articles dated the literal word `Unknown`.

Referenced by `snapshot_tests.rs::snapshot_no_home_site`.

See `basic-site/README.md` for the regeneration procedure.
