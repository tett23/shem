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
