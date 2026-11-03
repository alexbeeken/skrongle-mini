#!/usr/bin/env bash
# Turn a recorded demo .webm into an .mp4 with a song from songs/. Videos stay
# on this Mac and never go to GitHub.
#
#   finish-demo.sh <demo.webm> --out <dir>/demo.mp4 [song]   # into a PR's demo folder (references/manifest.md)
#   finish-demo.sh <demo.webm> <issue> <pr> <short-slug> [song]
#                                     # older form: ~/skrongle-demos/issue-<I>-pr-<N>-<slug>.mp4
#
# The song is picked at random from songs/*.py unless named (e.g. "gato").
# A song is a script taking `<out.wav> <seconds>`; drop a new one in songs/
# to add it. Prints the final path on stdout and the song on stderr. Also writes two frames next to the .webm
# (<demo>-f1.png, <demo>-f2.png) to look at before posting.
set -euo pipefail

webm=$1
if [[ "${2:-}" == --out ]]; then
  out=$3 song=${4:-}
else
  out="$HOME/skrongle-demos/issue-$2-pr-$3-$4.mp4" song=${5:-}
fi
here=$(cd "$(dirname "$0")" && pwd)
if [[ -z "$song" ]]; then
  songs=("$here"/songs/*.py)
  song=$(basename "${songs[RANDOM % ${#songs[@]}]}" .py)
fi
[[ -f "$here/songs/$song.py" ]] || { echo "no song $song in $here/songs" >&2; exit 1; }
echo "song: $song" >&2
mkdir -p "$(dirname "$out")"

secs=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$webm")
# Playwright's webm sometimes has no duration in the header; count frames instead.
if [[ -z "$secs" || "$secs" == "N/A" ]]; then
  secs=$(ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames,r_frame_rate -of csv=p=0 "$webm" \
    | awk -F, '{ split($1, r, "/"); print $2 / (r[1] / r[2]) }')
fi

track="${webm%.*}-track.wav"
python3 -I "$here/songs/$song.py" "$track" "$secs"

# Bring the track to a mean of about -20 dB, as on PRs #656 and #832. No fade here: each song
# shapes its own start and end (cringe opens with air horns over the title card).
mean=$(ffmpeg -hide_banner -i "$track" -af volumedetect -f null - 2>&1 | awk '/mean_volume/ { print $5 }')
gain=$(awk -v m="$mean" 'BEGIN { printf "%.1f", -20 - m }')

ffmpeg -y -loglevel error -i "$webm" -i "$track" -map 0:v -map 1:a \
  -vf fps=24 -c:v libx264 -preset slow -crf 30 -pix_fmt yuv420p \
  -af "volume=${gain}dB" -c:a aac -b:a 96k -shortest -movflags +faststart "$out"

for n in 1 2; do
  at=$(awk -v s="$secs" -v n="$n" 'BEGIN { printf "%.1f", s * n / 3 }')
  ffmpeg -y -loglevel error -ss "$at" -i "$out" -frames:v 1 "${webm%.*}-f$n.png"
done
rm -f "$track"
echo "$out"
