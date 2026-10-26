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
- **Few dependencies**: the parts it needs (Flate, the outlines of CFF and TrueType fonts, PNG) are written in this repository.

shem finds changes; it does not show that a rendering is correct.
A program and its test renderer written by the same hands can share the same misreading of the specification.
Check correctness with independent viewers, such as pdftoppm and PDFium.

## Scope by version

- **v0.1** reads only the subset of PDF that the current Golem writes. Images and figures are drawn as gray boxes with a cross, not decoded. Other PDFs will mostly be rejected with an error.
- **v0.x** follows what Golem writes, and widens the scope step by step toward v1.
- **v1** aims to be a general-purpose PDF rasterizer, for PDFs from other producers too.

The plan is in [docs/roadmap.md](docs/roadmap.md).

## Status

The scope of v0.1 is implemented.
The latest release candidate, v0.1.0-rc2, is on [Releases](https://github.com/tett23/shem/releases).
The design is decided in ADRs (Architecture Decision Records) under [docs/adr/](docs/adr/).
The specification is in [docs/specifications.md](docs/specifications.md).

## Installation

Download the archive for your platform and `SHA256SUMS` from [Releases](https://github.com/tett23/shem/releases), check it, and put `shem` on your `PATH`.

| Archive | Platform |
|---|---|
| `shem-<tag>-x86_64-unknown-linux-musl.tar.gz` | Linux x86-64 (statically linked) |
| `shem-<tag>-aarch64-unknown-linux-musl.tar.gz` | Linux aarch64 (statically linked) |
| `shem-<tag>-aarch64-apple-darwin.tar.gz` | macOS on Apple Silicon |

```sh
tag=v0.1.0-rc2
target=x86_64-unknown-linux-musl
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/shem-$tag-$target.tar.gz"
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/SHA256SUMS"
sha256sum --ignore-missing -c SHA256SUMS
tar -xzf "shem-$tag-$target.tar.gz"
```

On macOS, use `shasum -a 256 --ignore-missing -c SHA256SUMS`.
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

## Development

Install [adr-guard](https://github.com/tett23/adr-guard) v0.8.3 or later, and enable the git hooks in each clone.

```sh
cargo install --git https://github.com/tett23/adr-guard --tag v0.8.3
git config core.hooksPath .githooks
```

Run the tests with `cargo test`.
See [CONTRIBUTING.md](CONTRIBUTING.md) for how changes are made.

## License

[MIT](LICENSE)
