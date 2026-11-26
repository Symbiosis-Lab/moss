# no-home-site

Covers: builder edge case — no corresponding doc page

A site with content but **no home file** at the root: no `index.md`, no `readme.md`, no folder note. `html.rs` synthesizes the homepage instead, and until this fixture existed that arm was uncovered — `empty-site` is the only other fixture without a root home file and it contains no markdown at all, so nothing exercised the synthesized listing.

Also the regression fixture for asset folders being published as sections. With `[editor].attachment_folder = "./assets"`, the nested `assets/` folders get no page at all and the site-root `assets/` folders get the forwarding pages to `/` that any site's root `assets/` gets, never a listing, while each image's variants are still emitted, which proves the exclusion narrowed the index-page seed and not the scan walk.

Referenced by `snapshot_tests.rs::snapshot_no_home_site`.

See `basic-site/README.md` for the regeneration procedure.
