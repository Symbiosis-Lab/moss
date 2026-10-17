# video-embed-site

Covers: `moss-core::ast::dispatch_wikilink_embeds` — a video embed rendering by its own kind regardless of paragraph position.

A one-page site with a synthetic `clip.mp4` (2s, 320x240, generated with ffmpeg — see below), embedded twice: alone in its own paragraph, and sharing a paragraph with a caption joined by a soft line break. Pins the real end-to-end HTML for both: a `<video>` element in each case, never an `<img>` for the mid-paragraph one.

## Layout

- `input/` — source site, as a user would author it.
- `expected/` — build output from `moss build input` with `--no-plugins`. Canonical snapshot; tests diff against this.

`clip.mp4`'s bytes are not asserted (the snapshot suite compares non-HTML/CSS/JS/SVG/TXT/JSON/XML files by presence only — see `snapshot_tests.rs`'s `is_content_compared`), so its content doesn't need to stay in sync with what a real build's video pipeline (ffmpeg transcode, thumbnail, HLS ladder) produces on a given machine. The 320x240 clip fills the first two ladder boxes (240 px tall is more than the 234 px box of the second), so `expected/clip.hls/` holds its two-rung ladder; those files are present-only too. Only `index.html`'s markup is pinned, and it is what one cold build emits: the pages are rendered once more after the videos are encoded, so each `<video>` already carries the `<source src="/clip.hls/master.m3u8">` ladder form and the page links the player scripts (`data-hls`, `hls-boot`). There is no `width=`/`height=` on the video: dimension probing lands on a later build, and its absence is deliberate, not an omission to fix. The expected output therefore needs ffmpeg to have produced the ladder, as the present-only `clip.hls/`, `clip.thumb.jpg` and `clip.poster.jpg` already did.

## Regenerating `expected/`

```bash
SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests -- snapshot_video_embed_site
```

## Regenerating `clip.mp4`

```bash
ffmpeg -hide_banner -loglevel error -y -f lavfi -i "testsrc=size=320x240:rate=10:duration=2" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p input/clip.mp4
```
