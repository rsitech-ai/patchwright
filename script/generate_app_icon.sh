#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="$ROOT_DIR/Assets/PatchwrightIcon-source.png"
OUTPUT="$ROOT_DIR/Packaging/Patchwright.icns"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-icon.XXXXXX")"
OUTPUT_TMP="$ROOT_DIR/Packaging/.Patchwright.$$.icns"
trap 'rm -rf "$TMP_ROOT"; rm -f "$OUTPUT_TMP"' EXIT
ICONSET="$TMP_ROOT/Patchwright.iconset"
VERIFY_ICONSET="$TMP_ROOT/Verify.iconset"
mkdir -p "$ICONSET"

die() { echo "icon generation failed: $*" >&2; exit 65; }
[[ -f "$SOURCE" && ! -L "$SOURCE" ]] || die "missing regular raster master: $SOURCE"
[[ "$(/usr/bin/sips -g format "$SOURCE" 2>/dev/null | awk '/format:/{print $2}')" == png ]] \
  || die "raster master must be PNG"
[[ "$(/usr/bin/sips -g pixelWidth "$SOURCE" 2>/dev/null | awk '/pixelWidth:/{print $2}')" == 1024 ]] \
  || die "raster master width must be 1024"
[[ "$(/usr/bin/sips -g pixelHeight "$SOURCE" 2>/dev/null | awk '/pixelHeight:/{print $2}')" == 1024 ]] \
  || die "raster master height must be 1024"
/usr/bin/sips -g profile "$SOURCE" 2>/dev/null | grep -Fq 'sRGB' \
  || die "raster master must use an sRGB profile"

while IFS='|' read -r filename size; do
  /usr/bin/sips -z "$size" "$size" "$SOURCE" --out "$ICONSET/$filename" >/dev/null
done <<'EOF'
icon_16x16.png|16
icon_16x16@2x.png|32
icon_32x32.png|32
icon_32x32@2x.png|64
icon_128x128.png|128
icon_128x128@2x.png|256
icon_256x256.png|256
icon_256x256@2x.png|512
icon_512x512.png|512
icon_512x512@2x.png|1024
EOF

/usr/bin/iconutil --convert icns --output "$OUTPUT_TMP" "$ICONSET"
/usr/bin/iconutil --convert iconset --output "$VERIFY_ICONSET" "$OUTPUT_TMP"
[[ "$(find "$VERIFY_ICONSET" -type f -name '*.png' | wc -l | tr -d ' ')" == 10 ]] \
  || die "generated icon does not contain ten standard representations"
mv "$OUTPUT_TMP" "$OUTPUT"
echo "generated $OUTPUT"
