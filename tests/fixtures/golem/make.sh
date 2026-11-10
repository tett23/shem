#!/bin/sh
# Makes the PDF fixtures of Golem's output, the way Golem's output goldens
# make them (modules/golem-driver/test/OutputGoldenSpec.hs of Golem).
#
#   make.sh <golem source tree> <golem executable> <output directory>
#
# The source tree and the executable are those of the same Golem commit,
# recorded in tests/fixtures/README.md.
set -eu

[ $# -eq 3 ] || { echo "usage: make.sh <golem source tree> <golem executable> <output directory>" >&2; exit 64; }
src=$(cd "$1" && pwd)
golem=$2
out=$(mkdir -p "$3" && cd "$3" && pwd)
driver=$src/modules/golem-driver
work=$(mktemp -d "${TMPDIR:-/tmp}/shem-fixtures.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM

copy_fonts() {
  mkdir -p "$1/fonts"
  cp "$driver/test/fixtures/kanikosen/fonts/NotoSerifJP-Regular.otf" "$1/fonts/ja.otf"
  cp "$src/modules/golem-font/test/fonts/SourceSansPro-Regular.otf" "$1/fonts/latin.otf"
}

# build <project> <name> <golem build options...>
build() {
  project=$1
  name=$2
  shift 2
  (cd "$project" && "$golem" build --target pdf --no-notices "$@" > /dev/null)
  cp "$project"/build/pdf/*.pdf "$out/$name.pdf"
}

# sample: vertical, B6, with the page style of scripts/determinism/pdf-style.toml
cp -R "$driver/test/fixtures/sample" "$work/sample"
copy_fonts "$work/sample"
{ cat "$src/scripts/determinism/pdf-style.toml"; printf '\n[fonts.code]\nja = "fonts/ja.otf"\nlatin = "fonts/latin.otf"\n'; } > "$work/sample/style.toml"
build "$work/sample" sample

# sample-marks: the same with the Japanese crop marks (the Separation /Reg)
cp -R "$work/sample" "$work/sample-marks"
rm -rf "$work/sample-marks/build"
awk '{ print } /^writing-mode = "vertical"$/ { print "bleed = \"3mm\"" }' "$work/sample/style.toml" > "$work/sample-marks/style.toml"
build "$work/sample-marks" sample-marks --marks japanese

# figures: an SVG, a PDF and a JPEG figure
cp -R "$driver/test/fixtures/figures" "$work/figures"
copy_fonts "$work/figures"
build "$work/figures" figures

# truetype: the figures with the TrueType Source Serif (variable) as the
# Latin font, for CIDFontType2 and FontFile2
cp -R "$driver/test/fixtures/figures" "$work/truetype"
copy_fonts "$work/truetype"
cp "$src/modules/golem-font/test/fonts/SourceSerifVariable-Roman.ttf" "$work/truetype/fonts/latin.ttf"
awk '{ sub(/fonts\/latin\.otf/, "fonts/latin.ttf"); print }' "$driver/test/fixtures/figures/style.toml" > "$work/truetype/style.toml"
build "$work/truetype" truetype

# kanikosen: Golem's kanikosen fixture as it is (129 pages, vertical), for
# the measurements of speed (docs/adr/0013)
cp -R "$driver/test/fixtures/kanikosen" "$work/kanikosen"
build "$work/kanikosen" kanikosen

# boxes_project <directory> <writing mode> <binding>: a one-chapter book
# of paragraph boxes with round corners (Golem's docs/adr/0271), whose
# borders are strokes of paths: solid, dashed and dotted, and one broken
# by border-sides (docs/adr/0025)
boxes_project() {
  mkdir -p "$1/chapters"
  copy_fonts "$1"
  cat > "$1/manifest.toml" <<EOF
chapters = ["chapters/01-boxes.gm"]

[metadata]
id = "urn:uuid:00000000-0000-4000-8000-000000000025"
title = "角丸の囲みの見本"
language = "ja"
date = 2026-11-10
publisher = "shem"

[[metadata.creators]]
name = "shem"
role = "author"

[source]
paragraph-separator = "blank-line"

[targets.pdf]
EOF
  cat > "$1/style.toml" <<EOF
[page]
size = "A5"
binding = "$3"
writing-mode = "$2"

[page.grid]
font-size = "12Q"
characters = 26
lines = 20
line-pitch = "20H"
position = { top = "20mm", gutter = "18mm" }

[fonts.body]
ja = "fonts/ja.otf"
latin = "fonts/latin.otf"

[color.swatches]
red = { cmyk = [0, 100, 100, 0] }
blue = { cmyk = [100, 60, 0, 0] }
pale = { cmyk = [8, 0, 0, 0] }

[paragraph-styles.body]
first-line-indent = "1zw"

[paragraph-styles.solid]
first-line-indent = "0zw"
background = "pale"
padding = "0.5zw"
border = "blue"
border-width = "1pt"
border-radius = "1zw"

[paragraph-styles.dashed]
first-line-indent = "0zw"
padding = "0.5zw"
border = "red"
border-width = "1pt"
border-style = "dashed"
border-radius = "1.5zw"

[paragraph-styles.dotted]
first-line-indent = "0zw"
padding = "0.5zw"
border = "blue"
border-width = "1.5pt"
border-style = "dotted"
border-radius = "1zw"

[paragraph-styles.sides]
first-line-indent = "0zw"
padding = "0.5zw"
border = "red"
border-width = "1pt"
border-sides = ["before", "start"]
border-radius = "1zw"

[paragraph-styles.sides-dotted]
first-line-indent = "0zw"
padding = "0.5zw"
border = "blue"
border-width = "1.5pt"
border-style = "dotted"
border-sides = ["after", "start", "end"]
border-radius = "2zw"
EOF
  cat > "$1/chapters/01-boxes.gm" <<EOF
# 角丸の囲み

囲みの枠は、角を丸めると線で描かれる。

@style solid
実線の枠と背景を持つ囲みである。
四つの角が丸い。

@style dashed
破線の枠を持つ囲みである。
破線は辺と四分円の上で切れる。

@style dotted
点線の枠を持つ囲みである。
点は丸い線端の長さ 0 の破線である。

@style sides
前の辺と行の始まりの辺だけの枠である。
二つの辺のあいだの角だけが丸い。

@style sides-dotted
後の辺と行の両端の辺の点線の枠である。
途切れた辺の端は開いたパスの線端になる。

囲みの後の本文。
EOF
}

# boxes: the paragraph boxes with round corners set horizontally
boxes_project "$work/boxes" horizontal left
build "$work/boxes" boxes

# boxes-vertical: the same set vertically
boxes_project "$work/boxes-vertical" vertical right
build "$work/boxes-vertical" boxes-vertical

# complex-horizontal and complex-vertical: the two books that combine the
# features (Golem's docs/adr/0175), with a JPEG and SVG figures, as the
# goldens pdf-complex-horizontal.txt and pdf-complex-vertical.txt make
# them (docs/adr/0027)
for name in complex-horizontal complex-vertical; do
  cp -R "$driver/test/fixtures/$name" "$work/$name"
  copy_fonts "$work/$name"
  build "$work/$name" "$name"
done
