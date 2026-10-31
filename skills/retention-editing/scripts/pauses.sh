#!/bin/sh
# Prints the pauses in a video's sound as "start end length" (seconds), one per line.
# usage: pauses.sh <video> [from] [to] [noise dB, default -35] [shortest pause, default 0.25]
set -eu
f=$1 from=${2:-0} to=${3:-} noise=${4:--35} min=${5:-0.25}
ffmpeg -hide_banner -nostats -ss "$from" ${to:+-to "$to"} -i "$f" -vn \
  -af "silencedetect=noise=${noise}dB:d=$min" -f null - 2>&1 |
  awk -v off="$from" '
    /silence_start/ { s = $NF + off }
    /silence_end/   { printf "%.2f %.2f %.2f\n", s, $5 + off, $8 }'
