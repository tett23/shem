# JPEG fixtures

JPEG files for the tests of the decoder of `DCTDecode` streams
(docs/adr/0027, `src/jpeg.rs`), with the samples that libjpeg decodes them
to. Every file was made on 2026-11-10 with libjpeg-turbo 3.2.0 (the nix
package `libjpeg-turbo-3.2.0`: `cjpeg`, `djpeg` and `libjpeg.dylib`),
except `photo.jpg`.

## The files

All synthetic files are 19 × 13 pixels unless noted, so that a 16 × 16 MCU
leaves a partial MCU on the right and at the bottom. Their pixels are a
deterministic pattern (`enc.c` below). Quality 75, Huffman tables
optimized, `-dct int`.

| File | Frame | Components, sampling | Markers | Notes |
|---|---|---|---|---|
| `baseline-420.jpg` | SOF0, 8-bit | 3, 2×2 1×1 1×1 | JFIF | |
| `baseline-444.jpg` | SOF0, 8-bit | 3, 1×1 | JFIF | |
| `baseline-422.jpg` | SOF0, 8-bit | 3, 2×1 1×1 1×1 | JFIF | |
| `baseline-440.jpg` | SOF0, 8-bit | 3, 1×2 1×1 1×1 | JFIF | libjpeg-turbo interpolates (`h1v2_fancy_upsample`); docs/adr/0027 replicates |
| `baseline-411.jpg` | SOF0, 8-bit | 3, 4×1 1×1 1×1 | JFIF | replication in both |
| `tiny-420.jpg` | SOF0, 8-bit | 3, 2×2 1×1 1×1 | JFIF | 5 × 3 pixels: a downsampled width of 3, the smallest libjpeg interpolates |
| `progressive-420.jpg` | SOF2, 8-bit | 3, 2×2 1×1 1×1 | JFIF | the scans of `jpeg_simple_progression` |
| `progressive-444.jpg` | SOF2, 8-bit | 3, 1×1 | JFIF | |
| `extended-12bit-420.jpg` | SOF1, 12-bit | 3, 2×2 1×1 1×1 | JFIF | |
| `progressive-12bit-420.jpg` | SOF2, 12-bit | 3, 2×2 1×1 1×1 | JFIF | |
| `gray-12bit.jpg` | SOF1, 12-bit | 1 | JFIF | |
| `restart-420.jpg` | SOF0, 8-bit | 3, 2×2 1×1 1×1 | JFIF, DRI 2 | 35 × 21 pixels (3 × 2 MCUs) |
| `progressive-restart-420.jpg` | SOF2, 8-bit | 3, 2×2 1×1 1×1 | JFIF, DRI 2 | 35 × 21 pixels |
| `gray.jpg` | SOF0, 8-bit | 1 | JFIF | |
| `cmyk.jpg` | SOF0, 8-bit | 4, 1×1 | APP14 transform 0 | no colour transform |
| `ycck.jpg` | SOF0, 8-bit | 4, 2×2 1×1 1×1 1×1 | APP14 transform 2 | YCCK → CMYK |
| `ycbcr-adobe.jpg` | SOF0, 8-bit | 3, 2×2 1×1 1×1 | APP14 transform 1, no JFIF | |
| `rgb-adobe.jpg` | SOF0, 8-bit | 3, 1×1 | APP14 transform 0 | no colour transform; component ids `R`, `G`, `B` |
| `sequential-noninterleaved.jpg` | SOF0, 8-bit | 3, 1×1 | JFIF | three scans of one component each (`cjpeg -scans`) |
| `arithmetic.jpg` | SOF9 | 3 | JFIF | `cjpeg -arithmetic`: must be refused |
| `lossless.jpg` | SOF3 | 3 | APP14 | `cjpeg -lossless 1`: must be refused |
| `idct-8bit.jpg` | SOF0, 8-bit | 1 | JFIF | 48 × 8: six blocks of chosen coefficients, quantization table of ones (`coefs.c`) |
| `idct-12bit.jpg` | SOF1, 12-bit | 1 | JFIF | the same for 12 bits |
| `photo.jpg` | SOF0, 8-bit | 3, 2×2 1×1 1×1 | JFIF, APP1, APP13, DRI 1 | Golem's `figures/images/photo.jpg` (2 × 3 pixels) |

`photo.jpg` is a copy of
`modules/golem-driver/test/fixtures/figures/images/photo.jpg` of Golem at
`e313e3b05e5c0cdb240760974a6bb6f118c72961` (added by `7d3f9d69`; SHA-256
`675c0358609d02ae2a40f61036f7cdcf9d9e7a7472b0f0080e3648878e4eac84`).

## The reference decodings

`<name>.pnm` is the output of `djpeg -dct int -pnm <name>.jpg` (P5 or P6;
12-bit samples are two bytes, big-endian, with maxval 4095). Every one was
also made with `JSIMD_FORCENONE=1` and was the same bytes, so the SIMD
paths of libjpeg-turbo did not change the samples.

`cmyk.raw` and `ycck.raw` are the samples `jpeg_read_scanlines` gives for a
four-component file (CMYK, with YCCK converted by the library), because
djpeg converts CMYK to RGB when it writes a PNM. The format is a line
`width height components precision`, then the samples, one byte each.

`src/jpeg/vectors.rs` holds the coefficients of `idct-8bit.jpg` and
`idct-12bit.jpg` with the samples of their `.pnm`.

The decoder's tests compare each file with its reference decoding. Every
file matches sample for sample except `baseline-440.jpg`, whose chroma
libjpeg-turbo interpolates where docs/adr/0027 replicates.

## How they were made

Three small C programs against libjpeg-turbo 3.2.0 (not committed; this
describes them so that they can be written again):

- `enc.c`: `jpeg_write_scanlines` (or `jpeg12_write_scanlines`) of a
  pattern. 8-bit sample `(x × 11 + y × 13 + k × 83 + r) mod 256`, 12-bit
  `(x × 173 + y × 211 + k × 1301 + r) mod 4096`, where `k` is the
  component and `r` a value of the linear congruential generator
  `s = s × 1103515245 + 12345` (`r = ((s >> 16) & 0x7fff) mod 17`, or
  `mod 257` for 12 bits), seeded per file (1 for `baseline-420`, 2 for
  `baseline-444`, …, in the order of the table above, `gray-12bit` 18).
  The colour space is `jpeg_set_colorspace`'s, with `write_JFIF_header`
  and `write_Adobe_marker` set as the table says; `optimize_coding` true;
  `jpeg_simple_progression` for the progressive files;
  `restart_interval` 2 for the restart files.
- `coefs.c`: `jpeg_write_coefficients` of a 48 × 8 grayscale frame with
  `jpeg_add_quant_table` of 64 ones. Block 0 has only a DC term; the
  others have every coefficient drawn from the generator above
  (`(s >> 8) & 0xffffff`, seed 1), DC in ±1000 and AC in ±60 for 8 bits,
  DC in ±16000 and AC in ±1000 for 12 bits.
- `dump.c`: `jpeg_read_header`, `jpeg_start_decompress` with the defaults
  (`JDCT_ISLOW`, fancy upsampling, block smoothing) and
  `jpeg_read_scanlines`, writing the `.raw` format above.

The cjpeg files, from `djpeg -pnm baseline-444.jpg`:

```sh
printf '0;\n1;\n2;\n' > scans.txt
cjpeg -quality 75 -sample 1x1 -scans scans.txt -outfile sequential-noninterleaved.jpg in.ppm
cjpeg -quality 75 -arithmetic -outfile arithmetic.jpg in.ppm
cjpeg -lossless 1 -outfile lossless.jpg in.ppm
```
