#!/bin/sh
# Shared, reproducible FFmpeg release for CI fixtures and Docker media runtimes.
set -eu
version=9.0.2
sha256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
prefix=${1:?absolute installation prefix required}
profile=${2:-runtime}
case "$prefix" in /*) ;; *) exit 2 ;; esac
case "$profile" in runtime|fixtures) ;; *) exit 2 ;; esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
curl --fail --location --retry 3 --connect-timeout 20 --max-time 300 \
  "https://ffmpeg.org/releases/ffmpeg-${version}.tar.xz" -o "$work/source.tar.xz"
printf '%s  %s\n' "$sha256" "$work/source.tar.xz" | sha256sum --check --status
tar -xJf "$work/source.tar.xz" -C "$work"
cd "$work/ffmpeg-$version"
set -- --prefix="$prefix" --disable-autodetect --disable-network \
  --disable-programs --enable-ffmpeg --enable-ffprobe --disable-doc \
  --disable-debug --enable-small --disable-avdevice --disable-devices --disable-hwaccels
if [ "$profile" = fixtures ]; then
  set -- "$@" --enable-gpl --enable-libaom --enable-libopus --enable-libx264 --enable-libx265
else
  set -- "$@" --disable-swscale --disable-encoders --enable-encoder=webvtt --disable-filters
fi
./configure "$@"
make -j "${MEDIA_BUILD_JOBS:-2}"
make install
strip --strip-all "$prefix/bin/ffmpeg" "$prefix/bin/ffprobe"
mkdir -p "$prefix/share/media-probe"
"$prefix/bin/ffmpeg" -version > "$prefix/share/media-probe/version.txt"
if [ "$profile" = fixtures ]; then
  cp COPYING.GPLv2 "$prefix/share/media-probe/LICENSE"
else
  cp COPYING.LGPLv2.1 "$prefix/share/media-probe/LICENSE"
fi
"$prefix/bin/ffmpeg" -version | head -n 1 | grep -F "ffmpeg version $version"
"$prefix/bin/ffprobe" -version | head -n 1 | grep -F "ffprobe version $version"
