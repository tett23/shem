# shem

[![CI](https://github.com/tett23/shem/actions/workflows/ci.yml/badge.svg)](https://github.com/tett23/shem/actions/workflows/ci.yml)

shem is a PDF rasterizer under development.
It aims to render PDF pages to PNG images, with the same bytes on every machine.

[日本語の README](README.ja.md)

This English version is canonical.
README.ja.md is a machine translation of it.

## Overview

shem is a rasterizer for visual regression tests of PDF output.
It will render each page of a PDF to an image, so that a test can compare the images of two commits and find what changed in appearance.
It is made for [Golem](https://github.com/tett23/golem), a typesetting system, and can be used for any PDF that stays in the subset it reads.

These are its goals:

- **Deterministic**: the same input gives the same PNG, byte for byte, on x86-64 and aarch64, on Linux and on macOS. The rendering uses integer arithmetic only.
- **A subset of PDF**: shem reads the subset of PDF that Golem writes. It is not a general PDF viewer. It stops with an error at what it does not read, instead of drawing something else silently.
- **Few dependencies**: the parts it needs (Flate, the outlines of CFF and TrueType fonts, PNG) are written in this repository.

shem finds changes; it does not show that a rendering is correct.
A program and its test renderer written by the same hands can share the same misreading of the specification.
Check correctness with independent viewers, such as pdftoppm and PDFium.

## Status

Nothing is implemented yet.
The design is being decided in ADRs (Architecture Decision Records) under [docs/adr/](docs/adr/).
The specification is in [docs/specifications.md](docs/specifications.md).

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
