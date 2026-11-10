# shem

[![CI](https://github.com/tett23/shem/actions/workflows/ci.yml/badge.svg)](https://github.com/tett23/shem/actions/workflows/ci.yml)

shem is a PDF rasterizer under development.
It aims to render PDF pages to PNG images, with the same bytes on every machine.

[日本語の README](README.ja.md)

This English version is canonical.
README.ja.md is a machine translation of it.

## Overview

shem starts as a rasterizer for visual regression tests of PDF output.
It will render each page of a PDF to an image, so that a test can compare the images of two commits and find what changed in appearance.
Its first user is [Golem](https://github.com/tett23/golem), a typesetting system.

These are its goals:

- **Deterministic**: the same input gives the same PNG, byte for byte, on x86-64 and aarch64, on Linux and on macOS. The rendering uses integer arithmetic only.
- **Strict about what it reads**: shem stops with an error at what it does not read, instead of drawing something else silently.
- **Few dependencies**: the parts it needs (Flate, the outlines of CFF and TrueType fonts, PNG, JPEG) are written in this repository.

shem finds changes; it does not show that a rendering is correct.
A program and its test renderer written by the same hands can share the same misreading of the specification.
Check correctness with independent viewers, such as pdftoppm and PDFium.

## Scope by version

- **v0.1** reads only the subset of PDF that the current Golem writes. Images (Flate samples and JPEG, with an SMask) and figures (Form XObjects) are decoded and drawn. Other PDFs will mostly be rejected with an error.
- **v0.x** follows what Golem writes, and widens the scope step by step toward v1.
- **v1** aims to be a general-purpose PDF rasterizer, for PDFs from other producers too.

The plan is in [docs/roadmap.md](docs/roadmap.md).

## Status

The scope of v0.1 is implemented.
The latest release candidate, v0.1.0-rc9, is on [Releases](https://github.com/tett23/shem/releases).
The design is decided in ADRs (Architecture Decision Records) under [docs/adr/](docs/adr/).
The specification is in [docs/specifications.md](docs/specifications.md).

## Installation

Download the archive for your platform and `SHA256SUMS` from [Releases](https://github.com/tett23/shem/releases), check it, and put `shem` on your `PATH`.

| Archive | Platform |
|---|---|
| `shem-<tag>-x86_64-unknown-linux-musl.tar.gz` | Linux x86-64 (statically linked) |
| `shem-<tag>-aarch64-unknown-linux-musl.tar.gz` | Linux aarch64 (statically linked) |
| `shem-<tag>-aarch64-apple-darwin.tar.gz` | macOS on Apple Silicon |
| `shem.wasm` | WebAssembly, for browsers and Deno (the same name in every release) |

```sh
tag=v0.1.0-rc9
target=x86_64-unknown-linux-musl
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/shem-$tag-$target.tar.gz"
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/SHA256SUMS"
grep -F " shem-$tag-$target.tar.gz" SHA256SUMS > check && sha256sum -c check
tar -xzf "shem-$tag-$target.tar.gz"
```

On macOS, use `shasum -a 256 -c check` in place of `sha256sum -c check`.
`SHA256SUMS` sits in the same release as the archives, so it finds a broken download, not a release whose files were replaced.

The releases from v0.1.0-rc7 on carry attestations of build provenance.
With the [GitHub CLI](https://cli.github.com/), check that a file was built by the release workflow of this repository:

```sh
gh attestation verify "shem-$tag-$target.tar.gz" -R tett23/shem
```

These releases are immutable: their files and their tag cannot be changed after they are published.
The macOS binary is not signed or notarized; an archive downloaded with `curl` or `gh` carries no quarantine attribute.

To build from source instead, use Cargo:

```sh
cargo build --release
```

## Usage

Render every page of a PDF to PNG files in a directory:

```sh
shem render --dpi 150 input.pdf out
```

This writes `out/page-0001.png`, `out/page-0002.png` and so on.
`--format ppm` writes PPM files instead, whose bytes depend only on the pixels.
`--pages 2-5` renders only some pages.
Pages are rendered in parallel on every core; `--jobs <N>` sets how many at once, and the output does not depend on it.
If any page to render is outside the subset that shem reads, shem writes no file, prints the place to standard error and exits with 1.
`shem --help` describes every option and exit status.

To see where two renderings differ, compare their images:

```sh
shem diff before/page-0003.png after/page-0003.png diff.png
shem diff before after diffs
```

The first compares two files; the second compares the images of two directories by name, writes a diff image for each pair that differs, and lists the pages that differ, were added or were removed.
In a diff image, red pixels are darker in the after image, blue ones are lighter, green ones are only in one image, and an orange frame surrounds the differences.
The exit status is 0 if nothing differs, 1 if something does, and 2 on an error.

The library of the crate (`shem::render::Session`, `shem::parallel::run`) may change without a new version while shem is at v0.x.

### In the browser

`shem.wasm` renders pages to RGBA in a browser (a Web Worker) or in Deno.
It has no imports: `WebAssembly.instantiate(module, {})` loads it.
Its functions take and return 32-bit integers: pointers into its memory, lengths and numbers.

| Function | Arguments | Returns |
|---|---|---|
| `shem_alloc` | `len` | a pointer to `len` bytes of 0, to write the PDF to, or 0 |
| `shem_free` | `ptr` | nothing; frees what `shem_alloc` gave |
| `shem_open` | `ptr`, `len`, `dpi` | the number of a document (from 1), or 0 |
| `shem_page_count` | `doc` | the number of pages, or 0 for no such document |
| `shem_render` | `doc`, `page` | 1, or 0; renders page `page` (from 0) of the document |
| `shem_image_ptr`, `shem_image_width`, `shem_image_height` | `doc` | the RGBA of the last rendered page, and its size |
| `shem_close` | `doc` | nothing; frees the document and its image |
| `shem_error_ptr`, `shem_error_len` | none | the UTF-8 message of the last error |

```js
const { exports: shem } = await WebAssembly.instantiate(module, {});
const ptr = shem.shem_alloc(pdf.length);
new Uint8Array(shem.memory.buffer, ptr, pdf.length).set(pdf);
const doc = shem.shem_open(ptr, pdf.length, 144);
shem.shem_free(ptr);
if (doc === 0) throw new Error(message(shem));
for (let page = 0; page < shem.shem_page_count(doc); page++) {
  if (shem.shem_render(doc, page) === 0) throw new Error(message(shem));
  const width = shem.shem_image_width(doc), height = shem.shem_image_height(doc);
  const rgba = new Uint8ClampedArray(shem.memory.buffer, shem.shem_image_ptr(doc), width * height * 4);
  show(new ImageData(rgba.slice(), width, height));
}
shem.shem_close(doc);
```

Here `message` reads `shem_error_len()` bytes at `shem_error_ptr()`.
`shem_alloc` returns 0 when the bytes cannot be allocated; the message is then `cannot allocate <len> bytes`.
`shem_render` checks only the page it renders, so the first page can be shown before the others are read.
The messages of errors are those that `shem render` prints.
The image stays valid until the next `shem_render` or `shem_close` of the document.
A panic would be a trap (`WebAssembly.RuntimeError`); then load a new instance.

## Development

Install [adr-guard](https://github.com/tett23/adr-guard) v0.10.0 or later, and enable the git hooks in each clone.

```sh
cargo install --git https://github.com/tett23/adr-guard --tag v0.10.0
git config core.hooksPath .githooks
```

Run the tests with `cargo test`.
To test `shem.wasm`, build it with the Rust of `rust-toolchain.toml` and run the tests of Deno:

```sh
rustup toolchain install
rustup target add wasm32-unknown-unknown
sh scripts/ci/build-wasm.sh target/shem.wasm
SHEM_WASM=target/shem.wasm deno test --allow-read --allow-env wasm/test.ts
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for how changes are made.

## License

[MIT](LICENSE).
The JPEG decoder computes with the integer formulas of the Independent JPEG Group's libjpeg, written again here; see [NOTICE](NOTICE).
