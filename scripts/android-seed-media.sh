#!/usr/bin/env bash
# Puts a few photos and videos on the running emulator so backup, the library and
# the video player have something to chew on. Needs ffmpeg; no network.
set -euo pipefail
SDK="${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}"
ADB="$SDK/platform-tools/adb"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/Camera" "$WORK/Travel"

# Real faces from the repo fixtures (so People/semantic search have material) ...
cp "$HERE"/data/clooney*.jpg "$HERE"/data/stone*.jpg "$WORK/Camera/" 2>/dev/null || true
# ... plus synthetic stills with different aspect ratios.
n=1
for size in 1200x900 900x1200 1600x900 1080x1080; do
  for pattern in testsrc2 mandelbrot; do
    ffmpeg -y -loglevel error -f lavfi -i "$pattern=size=$size" -frames:v 1 \
      "$WORK/Travel/IMG_2026$(printf %04d "$n").jpg"; n=$((n + 1))
  done
done
ffmpeg -y -loglevel error -f lavfi -i testsrc2=size=1280x720:rate=30:duration=8 -f lavfi -i sine=frequency=440:duration=8 \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -movflags +faststart -shortest "$WORK/Camera/VID_20260001.mp4"
ffmpeg -y -loglevel error -f lavfi -i testsrc=size=720x1280:rate=30:duration=5 -f lavfi -i sine=frequency=660:duration=5 \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -movflags +faststart -shortest "$WORK/Camera/VID_20260002.mp4"

"$ADB" wait-for-device
"$ADB" push "$WORK/Camera" /sdcard/DCIM/ >/dev/null
"$ADB" push "$WORK/Travel" /sdcard/DCIM/ >/dev/null
"$ADB" shell 'for f in /sdcard/DCIM/Camera/* /sdcard/DCIM/Travel/*; do
  am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d "file://$f" >/dev/null; done'
echo "seeded: $(ls "$WORK/Camera" "$WORK/Travel" | grep -c .) files"
