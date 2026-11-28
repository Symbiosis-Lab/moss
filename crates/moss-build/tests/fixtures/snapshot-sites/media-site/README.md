# media-site

Covers: docs/media.md

Exercises the media pipeline (images, video, asset copies, nested media).

## Layout

This fixture is structured as a parent with sub-fixtures:

- `input/` + `expected/` — top-level media-site, covers `docs/media.md` examples.
- `pipeline-assets/input/` — asset pipeline edge cases (used by `pipeline_correctness.rs` `test_asset_pipeline`).
- `pipeline-nested/input/` — nested directory behavior (used by `test_nested_directory_structure` and `test_immersive_page_types`).
- `image-pages/input/` — image-specific page rendering.

Tests that consume the sub-fixtures call `setup_fixture("media-site/<name>")`.

See `basic-site/README.md` for the regeneration procedure.
