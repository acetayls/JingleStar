#!/bin/bash
# Génère les médias de test : contenu animé + carré repère de couleur en haut à gauche (400x400 à l'échelle).
F="/c/Program Files/Shutter Encoder/Library/ffmpeg.exe"
O="$1"; mkdir -p "$O"; cd "$O"
D=20
mk() { # nom couleur WxH fps codec-args...
  n=$1; c=$2; s=$3; r=$4; shift 4
  W=${s%x*}; B=$((W*400/1920))
  [ -f "$n" ] && return
  "$F" -hide_banner -loglevel error -y -f lavfi -i "testsrc2=s=$s:r=$r:d=$D" -f lavfi -i "sine=f=440:d=$D:r=48000" \
    -filter_complex "[0:v]noise=alls=12:allf=t,drawbox=x=0:y=0:w=$B:h=$B:color=$c:t=fill[v]" -map "[v]" -map 1:a -shortest "$@" "$n" && echo "ok $n" || echo "FAIL $n"
}
mk h264_1080p30.mp4 red 1920x1080 30 -c:v libx264 -preset fast -b:v 15M -pix_fmt yuv420p -c:a aac
mk h264_1080p60.mp4 green 1920x1080 60 -c:v libx264 -preset fast -b:v 25M -pix_fmt yuv420p -c:a aac
mk h264_4k30.mp4 blue 3840x2160 30 -c:v h264_nvenc -b:v 60M -pix_fmt yuv420p -c:a aac
mk hevc_4k30.mp4 yellow 3840x2160 30 -c:v hevc_nvenc -b:v 50M -pix_fmt yuv420p -tag:v hvc1 -c:a aac
mk hevc10_4k60.mp4 magenta 3840x2160 60 -c:v hevc_nvenc -profile:v main10 -b:v 80M -pix_fmt p010le -tag:v hvc1 -c:a aac
mk av1_4k30.mp4 cyan 3840x2160 30 -c:v av1_nvenc -b:v 40M -pix_fmt yuv420p -c:a aac
mk vp9_1080p30.webm white 1920x1080 30 -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -row-mt 1 -b:v 10M -c:a libopus
mk prores_1080p30.mov orange 1920x1080 30 -c:v prores_ks -profile:v 3 -pix_fmt yuv422p10le -c:a pcm_s16le
mk prores_4k30.mov purple 3840x2160 30 -c:v prores_ks -profile:v 3 -pix_fmt yuv422p10le -c:a pcm_s16le
mk hap_1080p30.mov lime 1920x1080 30 -c:v hap -format hap -c:a pcm_s16le
mk hap_4k30.mov pink 3840x2160 30 -c:v hap -format hap -c:a pcm_s16le
mk dnxhr_4k30.mov navy 3840x2160 30 -c:v dnxhd -profile:v dnxhr_hq -pix_fmt yuv422p -c:a pcm_s16le
"$F" -hide_banner -loglevel error -y -f lavfi -i "color=c=teal:s=3840x2160" -frames:v 1 still_4k.png && echo ok png
ls -la
