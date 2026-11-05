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
download_source() {
  # TLS resets (curl 35) are not retried by --retry alone. A second host also
  # avoids making every Rust/browser/Docker job depend on ffmpeg.org uptime.
  # Both hosts must supply the exact same pinned release archive.
  for source in \
    "https://ffmpeg.org/releases/ffmpeg-${version}.tar.xz" \
    "https://distfiles.macports.org/ffmpeg/ffmpeg-${version}.tar.xz"
  do
    printf 'Downloading FFmpeg %s from %s\n' "$version" "$source" >&2
    if curl --fail --location --proto '=https' --proto-redir '=https' \
      --retry 2 --retry-all-errors --retry-delay 2 --retry-max-time 60 \
      --connect-timeout 15 --max-time 120 \
      "$source" -o "$work/source.tar.xz"
    then
      if ! printf '%s  %s\n' "$sha256" "$work/source.tar.xz" | sha256sum --check --status
      then
        printf 'FFmpeg %s source checksum mismatch; refusing extraction.\n' "$version" >&2
        return 1
      fi
      return 0
    fi
    printf 'FFmpeg source unavailable from %s; trying the next source.\n' "$source" >&2
  done
  printf 'Unable to download the pinned FFmpeg %s archive.\n' "$version" >&2
  return 1
}
download_source
tar -xJf "$work/source.tar.xz" -C "$work"
cd "$work/ffmpeg-$version"
set -- --prefix="$prefix" --disable-autodetect --disable-network \
  --disable-programs --enable-ffmpeg --enable-ffprobe --disable-doc \
  --disable-debug --enable-small --disable-avdevice --disable-devices --disable-hwaccels
if [ "$profile" = fixtures ]; then
  # lavfi is an input device in libavdevice, not a libavformat demuxer.
  # Enable only this synthetic input for fixtures; keep physical devices disabled.
  set -- "$@" --enable-avdevice --enable-indev=lavfi \
    --enable-gpl --enable-libaom --enable-libopus --enable-libx264 --enable-libx265
else
  # Preview images are application derivatives, separate from playback/remux.
  # Keep only the JPEG/WebVTT encoders and the filters used by the Node.
  set -- "$@" --enable-libdav1d --disable-encoders --enable-encoder=webvtt,mjpeg \
    --disable-filters --enable-filter=fps,scale,format,tile
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
