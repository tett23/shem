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
