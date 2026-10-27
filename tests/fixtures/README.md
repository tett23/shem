# Fixtures

## golem/

PDFs written by Golem, for the tests of the subset of PDF that shem reads
(docs/adr/0002) and, later, for the goldens of the rendering.

| File | Contents |
|---|---|
| `sample.pdf` | Golem's `sample` fixture, set vertically on B6 with the page style of Golem's `scripts/determinism/pdf-style.toml`, as Golem's output golden `pdf-sample.txt` makes it |
| `sample-marks.pdf` | the same with the Japanese crop marks (`--marks japanese`, with `bleed = "3mm"`), for the Separation `/Reg` |
| `figures.pdf` | Golem's `figures` fixture: an SVG, a PDF and a JPEG figure, as the golden `pdf-figures.txt` makes it |
| `truetype.pdf` | the `figures` fixture with the TrueType Source Serif (variable) as the Latin font, for `CIDFontType2` and `FontFile2` |
| `kanikosen.pdf` | Golem's `kanikosen` fixture as it is (129 pages, vertical), for the measurements of speed and the bytes of many pages (docs/adr/0013) |

Two lists record what shem read and wrote before the changes of docs/adr/0013:

- `kanikosen.sha256`: the SHA-256 of every file of `shem render --dpi <72|150> --format <png|ppm>` of `kanikosen.pdf`, made with v0.1.0-rc2. `tests/kanikosen.rs` checks it.
- `operators.sha256`: the SHA-256 of the `Debug` form of the operators of every page of every PDF here. `tests/golem_fixtures.rs` checks it.

To write them again after an intended change, run the tests with `SHEM_UPDATE_GOLDEN=1`.

They were made by `make.sh` with Golem at commit
`1254092a2ed5a7fd29bb6f1affb53806cf76c694` (`golem 0.1.0-rc3`), the commit
whose output docs/adr/0002 surveyed:

```sh
git -C <golem> archive 1254092a2ed5a7fd29bb6f1affb53806cf76c694 | tar -x -C <tree>
(cd <tree> && cabal build exe:golem)
sh tests/fixtures/golem/make.sh <tree> "$(cd <tree> && cabal list-bin golem)" tests/fixtures/golem
```

Running `make.sh` twice gives the same bytes.

The PDFs embed subsets of these fonts, all under the SIL Open Font License
1.1, whose texts are in `golem/licenses/`:

- Noto Serif JP (Golem's `modules/golem-driver/test/fixtures/kanikosen/fonts/`): `LICENSE-NotoSerifJP.txt`
- Source Sans Pro and Source Serif (variable), from the test data of HarfBuzz 14.6.0 (Golem's `modules/golem-font/test/fonts/`): `OFL-SourceSansPro-SourceSerif.txt`

## flate/

zlib streams (`.zz`) and their uncompressed data (`.raw`), for the tests of
inflation (docs/adr/0002). They were made with the zlib module of Python 3
(zlib 1.2.12), with `compressobj(level, DEFLATED, 15, 9, strategy)`:

| File | Data | Level, strategy | First block |
|---|---|---|---|
| `stored` | 1000 bytes of a linear congruential generator (seed 1, bytes `(x >> 16) & 0xff` of `x = x * 1103515245 + 12345 mod 2^31`) | 0 | stored |
| `fixed` | `hello hello hello world` | 9, `Z_FIXED` | fixed Huffman |
| `dynamic` | 20000 bytes drawn from `eeeeeeeettttaaaoooiinnsshrdlu  \n` by the same generator (seed 3) | 9, default | dynamic Huffman |
| `empty` | no bytes | 9, default | fixed Huffman |
| `long` | 1000 bytes of `a` | 9, default | fixed Huffman |

zlib never uses a distance of 32768, so the tests build the streams for the
boundary values of lengths and distances from their bits.
