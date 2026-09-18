#!/usr/bin/env python3
"""Render selection.json. Use --fetch to download any missing source films."""
import argparse
import json
from pathlib import Path
import subprocess
from urllib.request import urlretrieve

root = Path(__file__).resolve().parents[1] / "site" / "scene5-loop"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--sources", type=Path, default=Path("/tmp/moss-film-sources"))
parser.add_argument("--fetch", action="store_true")
args = parser.parse_args()
edit = json.loads((root / "selection.json").read_text())
shots = edit["shots"]
fps = edit["fps"]
fade = edit["transition_seconds"]
closing = edit["loop_transition_seconds"]
inputs, filters, durations = [], [], []

for i, shot in enumerate(shots + [shots[0]]):
    source = args.sources / shot["file"]
    if not source.exists() and args.fetch:
        source.parent.mkdir(parents=True, exist_ok=True)
        partial = source.with_suffix(source.suffix + ".part")
        urlretrieve(shot["source_url"], partial)
        partial.replace(source)
    if not source.exists():
        parser.error(f"Missing {source}; provide --sources or use --fetch")
    duration = shot["end"] - shot["start"] if i < len(shots) else closing
    durations.append(duration)
    inputs += ["-ss", str(shot["start"]), "-t", str(duration + 1 / fps), "-i", str(source)]
    grade = shot["grade"]
    span = grade["white_in"] - grade["black_in"]
    assert span > 0, f"Invalid exposure range for {shot['file']}"
    tone = f"clip((val-{grade['black_in']})*{grade['white_out'] - grade['black_out']}/{span}+{grade['black_out']},{grade['black_out']},{grade['white_out']})"
    filters.append(
        f"[{i}:v]crop={shot['source_crop']},fps={fps},hue=s=0,lutrgb=r='{tone}':g='{tone}':b='{tone}',"
        "scale=1280:720:force_original_aspect_ratio=increase,"
        "crop=1280:720,setsar=1,settb=AVTB,"
        f"trim=duration={duration},setpts=PTS-STARTPTS[v{i}]"
    )

label, elapsed = "v0", durations[0]
for i in range(1, len(shots) + 1):
    transition = closing if i == len(shots) else fade
    filters.append(f"[{label}][v{i}]xfade=transition=fade:duration={transition}:offset={elapsed - transition:.6f}[x{i}]")
    label = f"x{i}"
    elapsed += durations[i] - transition
# The closing dissolve ends at the first shot's `closing` frame; begin there
# too, so replay continues the same brush stroke rather than jumping to black.
filters.append(f"[{label}]trim=start={closing}:end={elapsed:.6f},setpts=PTS-STARTPTS[out]")
out = root / "out"
out.mkdir(exist_ok=True)
video = out / (edit["name"] + ".mp4")
partial_video = video.with_suffix(".partial.mp4")
subprocess.run([
    "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *inputs,
    "-filter_complex_threads", "1", "-filter_complex", ";".join(filters),
    "-map", "[out]", "-an", "-c:v", "libx264", "-crf", "26",
    "-preset", "slow", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(partial_video),
], check=True)
partial_video.replace(video)
subprocess.run([
    "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-ss", "1",
    "-i", str(video), "-frames:v", "1", "-q:v", "3",
    str(out / (edit["name"] + "-poster.jpg")),
], check=True)
print(f"{video}: {elapsed - closing:.2f}s, {video.stat().st_size / 1e6:.2f} MB")
