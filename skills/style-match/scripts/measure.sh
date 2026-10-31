#!/bin/sh
# Measures a finished video's pacing and sound: length, cuts, seconds per shot, loudness.
# usage: measure.sh <video> [scene threshold 0-1, default 0.3]
set -eu
f=$1 t=${2:-0.3}
len=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$f")
cuts=$(ffmpeg -hide_banner -nostats -i "$f" -an \
  -vf "scale=320:-2,select='gt(scene,$t)',metadata=print:file=-" -f null - 2>/dev/null |
  awk -F'pts_time:' '/pts_time/ { printf "%s%.2f", sep, $2; sep = " " }')
n=$(echo "$cuts" | wc -w | tr -d ' ')
lufs=$(ffmpeg -hide_banner -nostats -i "$f" -vn -af ebur128=peak=true -f null - 2>&1 |
  awk '/I:/ { i = $2 } /Peak:/ { p = $2 } END { print i " LUFS, true peak " p " dBTP" }')
size=$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=s=x:p=0 "$f")
awk -v len="$len" -v n="$n" -v size="$size" -v lufs="$lufs" 'BEGIN {
  printf "size %s\nlength %.1fs\ncuts %d (%.1f a minute)\nseconds per shot %.1f\nloudness %s\n",
    size, len, n, n / len * 60, len / (n + 1), lufs }'
echo "cut times: $cuts"
