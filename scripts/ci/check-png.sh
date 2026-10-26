#!/bin/sh
# Checks the PNGs of shem with other implementations (docs/adr/0007):
# pngcheck checks their structure and CRCs, and ImageMagick decodes them
# to the same pixels as shem's own PPMs.
#
#   check-png.sh [shem executable]
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
shem=${1:-$root/target/release/shem}
work=$(mktemp -d "${TMPDIR:-/tmp}/shem-check-png.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
if command -v magick > /dev/null 2>&1; then
  im=magick
else
  im=convert
fi

status=0
checked=0
for pdf in "$root"/tests/fixtures/golem/*.pdf; do
  name=$(basename "$pdf" .pdf)
  "$shem" render --dpi 100 "$pdf" "$work/$name/png"
  "$shem" render --dpi 100 --format ppm "$pdf" "$work/$name/ppm"
  for png in "$work/$name/png"/*.png; do
    page=$(basename "$png" .png)
    ppm="$work/$name/ppm/$page.ppm"
    if ! pngcheck -q "$png"; then
      echo "check-png: pngcheck rejects $name/$page.png" >&2
      status=1
    fi
    "$im" "$png" -type TrueColor -depth 8 "ppm:$work/decoded.ppm"
    # The second line of shem's PPM is "<width> <height>"
    size=$(sed -n 2p "$ppm")
    bytes=$(( ${size% *} * ${size#* } * 3 ))
    tail -c "$bytes" "$work/decoded.ppm" > "$work/a"
    tail -c "$bytes" "$ppm" > "$work/b"
    if ! cmp -s "$work/a" "$work/b"; then
      echo "check-png: ImageMagick decodes $name/$page.png to other pixels" >&2
      status=1
    fi
    checked=$((checked + 1))
  done
done
echo "check-png: $checked pages checked"
exit "$status"
