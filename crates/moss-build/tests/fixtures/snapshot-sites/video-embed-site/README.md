# video-embed-site

Covers: `moss-core::ast::dispatch_wikilink_embeds` — a video embed rendering by its own kind regardless of paragraph position.

A one-page site with a synthetic `clip.mp4` (2s, 320x240, generated with ffmpeg — see below), embedded twice: alone in its own paragraph, and sharing a paragraph with a caption joined by a soft line break. Pins the real end-to-end HTML for both: a `<video>` element in each case, never an `<img>` for the mid-paragraph one.

## Layout

- `input/` — source site, as a user would author it.
- `expected/` — build output from `moss build input` with `--no-plugins`. Canonical snapshot; tests diff against this.

`clip.mp4`'s bytes are not asserted (the snapshot suite compares non-HTML/CSS/JS/SVG/TXT/JSON/XML files by presence only — see `snapshot_tests.rs`'s `is_content_compared`), so its content doesn't need to stay in sync with what a real build's video pipeline (ffmpeg transcode, thumbnail, HLS ladder) produces on a given machine. Only `index.html`'s markup is pinned, and only what a single first build emits: no `width=`/`height=` (dimension probing lands on a later build) and no HLS `<source>` ladder (built off a later, cached transform pass) — both are deliberately absent from `expected/index.html` for that reason, not omissions to fix.

## Regenerating `expected/`

```bash
SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests -- snapshot_video_embed_site
```

## Regenerating `clip.mp4`

```bash
ffmpeg -hide_banner -loglevel error -y -f lavfi -i "testsrc=size=320x240:rate=10:duration=2" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p input/clip.mp4
```
