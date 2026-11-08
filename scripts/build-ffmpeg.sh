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
# Chromaprint (audio fingerprints for intro/credits detection) is built statically
# with its bundled KissFFT: the distribution package drags a whole second libavcodec.
chromaprint_version=1.5.1
chromaprint_sha256=a1aad8fa3b8b18b78d3755b3767faff9abb67242e01b478ec9a64e190f335e1c
build_chromaprint() {
  curl --fail --location --proto '=https' --proto-redir '=https' \
    --retry 2 --retry-all-errors --retry-delay 2 --retry-max-time 60 \
    --connect-timeout 15 --max-time 120 \
    "https://github.com/acoustid/chromaprint/releases/download/v${chromaprint_version}/chromaprint-${chromaprint_version}.tar.gz" \
    -o "$work/chromaprint.tar.gz"
  if ! printf '%s  %s\n' "$chromaprint_sha256" "$work/chromaprint.tar.gz" | sha256sum --check --status
  then
    printf 'Chromaprint %s source checksum mismatch; refusing extraction.\n' "$chromaprint_version" >&2
    return 1
  fi
  tar -xzf "$work/chromaprint.tar.gz" -C "$work"
  cmake -S "$work/chromaprint-${chromaprint_version}" -B "$work/chromaprint-build" \
    -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBUILD_TOOLS=OFF -DBUILD_TESTS=OFF \
    -DFFT_LIB=kissfft -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DCMAKE_INSTALL_PREFIX="$work/chromaprint-prefix"
  cmake --build "$work/chromaprint-build" --parallel "${MEDIA_BUILD_JOBS:-2}"
  cmake --install "$work/chromaprint-build"
}
download_source
tar -xJf "$work/source.tar.xz" -C "$work"
if [ "$profile" = runtime ]; then
  build_chromaprint
  PKG_CONFIG_PATH="$work/chromaprint-prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  export PKG_CONFIG_PATH
fi
cd "$work/ffmpeg-$version"
set -- --prefix="$prefix" --disable-autodetect --disable-network \
  --disable-programs --enable-ffmpeg --enable-ffprobe --disable-doc \
  --disable-debug --enable-small --disable-avdevice --disable-devices --disable-hwaccels
if [ "$profile" = fixtures ]; then
  # lavfi is an input device in libavdevice, not a libavformat demuxer.
  # Enable only this synthetic input for fixtures; keep physical devices disabled.
  set -- "$@" --enable-avdevice --enable-indev=lavfi \
    --enable-gpl --enable-libaom --enable-libopus --enable-libx264 --enable-libx265 --enable-libwebp
else
  # Preview images are application derivatives, separate from playback/remux.
  # Keep only the WebP/JPEG/WebVTT encoders and the filters used by the Node.
  # Audio fingerprints (intro detection) need the chromaprint muxer, the PCM encoder it
  # consumes and the audio resampler that downmixes to 11 kHz mono.
  set -- "$@" --enable-libdav1d --enable-libwebp --disable-encoders --enable-encoder=webvtt,mjpeg,libwebp,pcm_s16le \
    --disable-filters --enable-filter=fps,scale,format,tile,aresample,aformat,anull \
    --enable-chromaprint --enable-muxer=chromaprint \
    --extra-cflags="-I$work/chromaprint-prefix/include" \
    --extra-ldflags="-L$work/chromaprint-prefix/lib" --extra-libs="-lstdc++ -lm"
fi
./configure "$@"
make -j "${MEDIA_BUILD_JOBS:-2}"
make install
strip --strip-all "$prefix/bin/ffmpeg" "$prefix/bin/ffprobe"
if [ "$profile" = runtime ]; then
  # Intro detection silently never runs without this muxer: fail the image build instead.
  "$prefix/bin/ffmpeg" -hide_banner -muxers | grep -qw chromaprint
  # Native Trickplay sheets default to WebP: fail the image build if libwebp is missing.
  "$prefix/bin/ffmpeg" -hide_banner -encoders | grep -qw libwebp
fi
mkdir -p "$prefix/share/media-probe"
"$prefix/bin/ffmpeg" -version > "$prefix/share/media-probe/version.txt"
if [ "$profile" = fixtures ]; then
  cp COPYING.GPLv2 "$prefix/share/media-probe/LICENSE"
else
  cp COPYING.LGPLv2.1 "$prefix/share/media-probe/LICENSE"
fi
"$prefix/bin/ffmpeg" -version | head -n 1 | grep -F "ffmpeg version $version"
"$prefix/bin/ffprobe" -version | head -n 1 | grep -F "ffprobe version $version"
