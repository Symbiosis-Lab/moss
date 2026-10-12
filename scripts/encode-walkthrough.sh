#!/usr/bin/env bash
# Encodes a screen recording as a walkthrough video for the documentation site.
#
#   scripts/encode-walkthrough.sh "/path/to/new folder.mov"            # silent, for ![[new folder.mp4|loop|100%|Caption]]
#   scripts/encode-walkthrough.sh --audio "/path/to/Intro to moss.mp4" # keeps the sound, for a video with controls
#
# Writes site/assets/animations/<name>.mp4. The numbers are chosen so that the build publishes
# these exact bytes instead of encoding them again. A build leaves alone an H.264 file of at
# most 30 frames a second that fits one of its delivery sizes exactly; the largest is a
# 1920x1080 box, so the picture is scaled to fill that box on one side. A file that falls
# between two sizes, or runs faster, is re-encoded at a fixed bitrate, which for a screen
# recording is several times larger and no sharper.
set -euo pipefail

audio=(-an)
if [ "${1:-}" = "--audio" ]; then audio=(-c:a aac -b:a 96k -ac 2); shift; fi
src="${1:?usage: encode-walkthrough.sh [--audio] <recording>}"
name="$(basename "${src%.*}")"
out="$(cd "$(dirname "$0")/.." && pwd)/site/assets/animations/$name.mp4"

ffmpeg -v error -y -i "$src" "${audio[@]}" -sn -dn -map_chapters -1 \
  -vf "fps=30,scale=w=1920:h=1080:force_original_aspect_ratio=decrease:force_divisible_by=2:flags=lanczos,setsar=1" \
  -c:v libx264 -profile:v high -preset veryslow -crf 23 -pix_fmt yuv420p \
  -movflags +faststart "$out"

echo "$out ($(du -h "$out" | cut -f1 | tr -d ' '))"
