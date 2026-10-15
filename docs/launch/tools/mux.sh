#!/bin/bash
# Adds the licensed track to a silent picture. Copies the video stream, so it costs almost nothing. Never plays audio.
#   docs/launch/tools/mux.sh picture.mp4 cut.wav out.mp4
# The cut comes from MUSIC.md ("The exact edit"). The track itself never goes into git.
set -euo pipefail
pic="$1"; aud="$2"; out="$3"
pd=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$pic"); ad=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$aud")
echo "picture ${pd}s, audio ${ad}s"
nice -n 15 ffmpeg -hide_banner -nostats -y -i "$pic" -i "$aud" -map 0:v:0 -map 1:a:0 -c:v copy -c:a aac -b:a 256k -ar 48000 -movflags +faststart -shortest "$out"
ffprobe -v error -show_entries stream=codec_name,width,height,r_frame_rate,duration -of default=nw=1 "$out"
