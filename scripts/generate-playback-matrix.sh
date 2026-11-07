#!/bin/sh
# Synthetic playback matrix: AV1 8/10-bit x MP4/MKV/WebM x Opus stereo/5.1, each with a French
# (440 Hz) and an English (1050 Hz) track so a signal check can tell which track is audible.
# Output: $1 (default tests/fixtures/generated/matrix). Needs ffmpeg with libaom-av1 and libopus.
# Never overwrites an existing file. The media are generated test patterns, not real films:
# results on them say nothing about a NAS, a real encode or a personal library.
set -eu
out="${1:-tests/fixtures/generated/matrix}"
seconds="${MATRIX_SECONDS:-12}"
mkdir -p "$out"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
ff() { ffmpeg -v error -nostdin "$@"; }

for depth in 8 10; do
  pix=yuv420p; [ "$depth" = 10 ] && pix=yuv420p10le
  [ -f "$work/v$depth.mkv" ] || ff -f lavfi -i "testsrc2=size=320x180:rate=24" -t "$seconds" -an \
    -c:v libaom-av1 -cpu-used 8 -crf 40 -b:v 0 -g 24 -pix_fmt "$pix" "$work/v$depth.mkv"
done
for layout in stereo 5.1; do
  tag=$(echo "$layout" | tr -d .)
  for lang in fr en; do
    hz=440; [ "$lang" = en ] && hz=1050
    bitrate=96k; [ "$layout" = 5.1 ] && bitrate=256k
    ff -f lavfi -i "sine=frequency=$hz:sample_rate=48000" -t "$seconds" -af "aformat=channel_layouts=$layout" \
      -c:a libopus -b:a "$bitrate" -mapping_family "$([ "$layout" = 5.1 ] && echo 1 || echo 0)" "$work/a-$lang-$tag.mka"
  done
done

for depth in 8 10; do
  for layout in stereo 5.1; do
    tag=$(echo "$layout" | tr -d .)
    for ext in mp4 mkv webm; do
      file="$out/av1-${depth}bit-opus-$tag.$ext"
      extra=""; [ "$ext" = mp4 ] && extra="-movflags +faststart"
      [ -f "$file" ] && { echo "keep $file"; continue; }
      ff -i "$work/v$depth.mkv" -i "$work/a-fr-$tag.mka" -i "$work/a-en-$tag.mka" \
        -map 0:v -map 1:a -map 2:a -c copy \
        -metadata:s:a:0 language=fra -metadata:s:a:1 language=eng \
        -disposition:a:0 default -disposition:a:1 0 \
        $extra "$file"
      echo "wrote $file"
    done
  done
done
