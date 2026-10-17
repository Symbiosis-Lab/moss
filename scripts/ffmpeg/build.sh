#!/usr/bin/env bash
# Builds the static ffmpeg and ffprobe that moss downloads on macOS.
#
# Usage: scripts/ffmpeg/build.sh <arm64|x86_64> <out-dir>
#
# Writes into <out-dir>: ffmpeg-macos-<arch>.zip and ffprobe-macos-<arch>.zip
# (each holding one binary), the two source tarballs, and the license texts.
#
# FFMPEG_WORK_DIR selects the scratch directory for sources and build trees.
# It defaults to a fresh directory under $RUNNER_TEMP or $TMPDIR. A run that
# finds a finished stage in it skips that stage, so an interrupted build
# resumes where it stopped.
#
# FFmpeg is pinned by version and SHA-256. x264 is pinned by git commit: the
# commit id names the exact tree, while forge archive tarballs are not
# guaranteed to keep the same bytes. Bump a pin, commit and hash together.
set -euo pipefail

FFMPEG_VERSION=7.1.5
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SHA256=de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f
X264_COMMIT=b35605ace3ddf7c1a5d67a2eb553f034aef41d55
X264_REPO=https://code.videolan.org/videolan/x264.git
MIN_MACOS=12.0

usage() {
  echo "usage: $0 <arm64|x86_64> <out-dir>" >&2
  exit 2
}

[ $# -eq 2 ] || usage
ARCH=$1
case "$ARCH" in
  arm64 | x86_64) ;;
  *) usage ;;
esac
[ "$(uname -s)" = Darwin ] || {
  echo "build.sh builds macOS binaries and must run on macOS" >&2
  exit 1
}
for tool in clang make pkg-config shasum otool vtool lipo zip nasm git; do
  command -v "$tool" >/dev/null || {
    echo "missing tool: $tool" >&2
    exit 1
  }
done

HOST_ARCH=$(uname -m)
JOBS=$(sysctl -n hw.ncpu)
OUT=$(mkdir -p "$2" && cd "$2" && pwd)
WORK=${FFMPEG_WORK_DIR:-$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ffmpeg-build.XXXXXX")}
mkdir -p "$WORK/dl" "$WORK/$ARCH"

export MACOSX_DEPLOYMENT_TARGET=$MIN_MACOS
ARCH_FLAGS="-arch $ARCH -mmacosx-version-min=$MIN_MACOS"

sha256_of() {
  shasum -a 256 "$1" | awk '{print $1}'
}

# fetch <url> <sha256> <dest>: download once, verify, keep only the verified file.
fetch() {
  local url=$1 sha=$2 dest=$3
  if [ -f "$dest" ] && [ "$(sha256_of "$dest")" = "$sha" ]; then
    return 0
  fi
  curl -fsSL --retry 3 -o "$dest.part" "$url"
  if [ "$(sha256_of "$dest.part")" != "$sha" ]; then
    rm -f "$dest.part"
    echo "SHA-256 mismatch for $url" >&2
    exit 1
  fi
  mv "$dest.part" "$dest"
}

# extract <archive> <dir>: unpack with the top-level directory stripped.
extract() {
  local archive=$1 dir=$2
  if [ ! -f "$dir/.extracted" ]; then
    rm -rf "$dir"
    mkdir -p "$dir"
    tar -xf "$archive" -C "$dir" --strip-components=1
    touch "$dir/.extracted"
  fi
}

FFMPEG_TARBALL="$WORK/dl/ffmpeg-${FFMPEG_VERSION}.tar.xz"
fetch "$FFMPEG_URL" "$FFMPEG_SHA256" "$FFMPEG_TARBALL"

X264_SRC="$WORK/$ARCH/x264-src"
X264_BUILD="$WORK/$ARCH/x264-build"
X264_PREFIX="$WORK/$ARCH/x264-prefix"
FF_SRC="$WORK/$ARCH/ffmpeg-src"
FF_BUILD="$WORK/$ARCH/ffmpeg-build"

# Check out x264 at the pinned commit. A checkout already at that commit is
# kept, so a resumed run does not refetch it.
if [ "$(git -C "$X264_SRC" rev-parse HEAD 2>/dev/null || true)" != "$X264_COMMIT" ]; then
  rm -rf "$X264_SRC"
  mkdir -p "$X264_SRC"
  git -C "$X264_SRC" init -q
  git -C "$X264_SRC" fetch -q --depth 1 "$X264_REPO" "$X264_COMMIT"
  git -C "$X264_SRC" checkout -q --detach FETCH_HEAD
  [ "$(git -C "$X264_SRC" rev-parse HEAD)" = "$X264_COMMIT" ] || {
    echo "x264 checkout is not commit $X264_COMMIT" >&2
    exit 1
  }
fi

# x264 is a static library, so FFmpeg links it in and the binary depends on
# nothing outside the system. --host is needed only when cross-compiling.
if [ ! -f "$X264_PREFIX/lib/libx264.a" ]; then
  rm -rf "$X264_BUILD"
  mkdir -p "$X264_BUILD"
  X264_HOST=()
  if [ "$ARCH" != "$HOST_ARCH" ]; then
    X264_HOST=(--host="$ARCH-apple-darwin")
  fi
  (
    cd "$X264_BUILD"
    "$X264_SRC/configure" \
      --prefix="$X264_PREFIX" \
      --enable-static --disable-shared --disable-cli --enable-pic \
      ${X264_HOST[@]+"${X264_HOST[@]}"} \
      --extra-cflags="$ARCH_FLAGS" --extra-ldflags="$ARCH_FLAGS"
    make -j"$JOBS"
    make install
  )
fi

# The FFmpeg configure line is the one the release notes record. Keep it in
# step with those notes.
CROSS=()
if [ "$ARCH" != "$HOST_ARCH" ]; then
  CROSS=(--enable-cross-compile)
fi
extract "$FFMPEG_TARBALL" "$FF_SRC"
mkdir -p "$FF_BUILD"
if [ ! -f "$FF_BUILD/config.h" ]; then
  (
    cd "$FF_BUILD"
    PKG_CONFIG_PATH= PKG_CONFIG_LIBDIR="$X264_PREFIX/lib/pkgconfig" \
      "$FF_SRC/configure" \
      --enable-gpl --enable-libx264 \
      --disable-autodetect --disable-network --disable-doc --disable-ffplay \
      --enable-static --disable-shared --pkg-config-flags=--static \
      --arch="$ARCH" --target-os=darwin --cc=clang \
      ${CROSS[@]+"${CROSS[@]}"} \
      --extra-cflags="-I$X264_PREFIX/include $ARCH_FLAGS" \
      --extra-ldflags="-L$X264_PREFIX/lib $ARCH_FLAGS"
  )
fi
(cd "$FF_BUILD" && make -j"$JOBS" ffmpeg ffprobe)

BIN="$WORK/$ARCH/bin"
mkdir -p "$BIN"
cp "$FF_BUILD/ffmpeg" "$FF_BUILD/ffprobe" "$BIN/"
strip -x "$BIN/ffmpeg" "$BIN/ffprobe"

# Every binary must be this architecture only, link nothing outside the
# system library paths, and declare the minimum macOS version.
for name in ffmpeg ffprobe; do
  bin="$BIN/$name"
  [ "$(lipo -archs "$bin")" = "$ARCH" ] || {
    echo "$name is not a single-architecture $ARCH binary" >&2
    exit 1
  }
  if otool -L "$bin" | tail -n +2 | awk '{print $1}' | grep -v -E '^(/usr/lib/|/System/)' >/dev/null; then
    echo "$name links a library outside /usr/lib and /System:" >&2
    otool -L "$bin" >&2
    exit 1
  fi
  minos=$(vtool -show-build "$bin" | awk '$1 == "minos" {print $2}' | sort -u)
  [ "$minos" = "$MIN_MACOS" ] || {
    echo "$name minos is '$minos', expected $MIN_MACOS" >&2
    exit 1
  }
done

rm -f "$OUT/ffmpeg-macos-$ARCH.zip" "$OUT/ffprobe-macos-$ARCH.zip"
(cd "$BIN" && zip -X -q "$OUT/ffmpeg-macos-$ARCH.zip" ffmpeg)
(cd "$BIN" && zip -X -q "$OUT/ffprobe-macos-$ARCH.zip" ffprobe)

cp "$FFMPEG_TARBALL" "$OUT/"
git -C "$X264_SRC" archive --format=tar.gz --prefix="x264-$X264_COMMIT/" HEAD > "$OUT/x264-$X264_COMMIT.tar.gz"
cp "$FF_SRC/COPYING.GPLv2" "$OUT/ffmpeg-COPYING.GPLv2"
cp "$FF_SRC/LICENSE.md" "$OUT/ffmpeg-LICENSE.md"
cp "$X264_SRC/COPYING" "$OUT/x264-COPYING"

echo "built $ARCH into $OUT"
ls -l "$OUT"
