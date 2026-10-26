# Contributing to shem

[日本語](CONTRIBUTING.ja.md)

This English version is canonical.
CONTRIBUTING.ja.md is a machine translation of it.

## Development environment

Use the stable Rust toolchain.
Install [adr-guard](https://github.com/tett23/adr-guard) v0.8.3 or later, and enable the git hooks after cloning.
The setting is not stored in the repository, so run it in every clone.

```sh
cargo install --git https://github.com/tett23/adr-guard --tag v0.8.3
git config core.hooksPath .githooks
```

CI runs these checks on Linux (x86-64 and aarch64) and on macOS (aarch64):

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

## How a change is made

1. Write an ADR and have it reviewed. Do not commit the ADR before the review.
2. Write the tests of the ADR's test design, and see them fail.
3. Implement the change, and make the tests pass.
4. Update docs/specifications.md and the README in the same pull request.
5. Push a branch and open a pull request.
6. When CI passes, merge it into `main` with a merge commit (`--no-ff`).

Fixes of typos and of documents that do not change the specification need no ADR.

## Scope

v0.1 reads only the subset of PDF that the current Golem writes.
v1 aims to be a general-purpose PDF rasterizer.
A change that widens the scope, whether to follow Golem or to move toward v1, starts with an ADR.
The plan of each version is in [docs/roadmap.md](docs/roadmap.md).

## ADRs

- ADRs are in `docs/adr/`, named like `0001-scope-and-determinism.md`: a sequential number and a readable kebab-case name.
- An ADR has exactly one status line, which begins with `ステータス:`.
- A committed ADR does not change, except for its status line and its links. adr-guard enforces this in the pre-commit hook and in CI. To change a decision, write a new ADR that supersedes the old one.
- To discard an ADR, set its status to `破棄` and write the reason in docs/specifications.md.
- An ADR refers to source code by a GitHub URL that contains a commit hash. `main` is never rebased, so such a URL does not change.
- An ADR lists the references used for its decision at its end.
- After writing an ADR or a document, run `adr-guard links --fix` to add links to ADR references.

## Languages

- Commit messages, release notes and the CHANGELOG are written in English.
- ADRs and docs/specifications.md are written in Japanese.
- README.md and CONTRIBUTING.md are written in English, and the English version is canonical. README.ja.md and CONTRIBUTING.ja.md are machine translations of them. A commit that changes an English version updates its translation in the same commit.

## Code

- Prefer a functional style in Rust.
- Do not add a dependency for what can be written here without license problems. Write it here, with tests as thorough as those of the original.
- The rendering uses integer and fixed-point arithmetic only, so that its output is the same on every machine. Do not use floating-point numbers on the path from the input to the pixels.
- Do not rerun a flaky test until it passes. Find the cause and fix it.

## Git

- Do not rebase `main`. A branch may be rebased until it is merged.
- Merge with `--no-ff`.
- Tags that begin with `v` are releases. A tag such as `v0.1.0-rc1` is a pre-release.
- Release notes go to GitHub Releases, grouped as Added, Changed, Removed and Fixed. Each change names its pull request and its ADR, with absolute links to ADRs.
