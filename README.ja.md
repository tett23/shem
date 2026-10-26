# shem

[![CI](https://github.com/tett23/shem/actions/workflows/ci.yml/badge.svg)](https://github.com/tett23/shem/actions/workflows/ci.yml)

shem は、開発中の PDF のラスタライザである。
PDF のページを PNG の画像に描き、どの機械でも同じバイト列を出すことを目指す。

[English README](README.md)

この文書は [README.md](README.md)（英語）の機械翻訳である。
英語版を正とし、内容が食い違うときは英語版に従う。

## 概要

shem は、PDF の出力の見た目の回帰テストのためのラスタライザである。
PDF の各ページを画像に描き、テストが二つのコミットの画像を比べて見た目の変化を見つけられるようにする予定である。
組版システムの [Golem](https://github.com/tett23/golem) のために作るが、shem が読む部分集合に収まる PDF なら、何にでも使える。

目標は次のとおりである。

- **決定性**：同じ入力から、x86-64 でも aarch64 でも、Linux でも macOS でも、バイト単位で同じ PNG を出す。描画には整数の演算だけを使う。
- **PDF の部分集合**：shem は、Golem が書く PDF の部分集合を読む。汎用の PDF のビューアーではない。読まないものに出会うと、黙ってほかのものを描くのではなく、エラーで止まる。
- **少ない依存**：必要な部品（Flate、CFF と TrueType のフォントのアウトライン、PNG）は、このリポジトリの中で書く。

shem は変化を見つけるものであり、描いた結果が正しいことを示すものではない。
同じ書き手が作ったプログラムとテスト用のラスタライザは、仕様の同じ読み違いを共有しうる。
正しさは、pdftoppm や PDFium など、独立したビューアーで確かめる。

## 状態

まだ何も実装していない。
設計は [docs/adr/](docs/adr/) の ADR（Architecture Decision Record）で決めているところである。
仕様は [docs/specifications.md](docs/specifications.md) にある。

## 開発

[adr-guard](https://github.com/tett23/adr-guard) の v0.8.3 以降をインストールし、クローンごとに git の hook を有効にする。

```sh
cargo install --git https://github.com/tett23/adr-guard --tag v0.8.3
git config core.hooksPath .githooks
```

テストは `cargo test` で実行する。
変更の進め方は [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) を参照。

## ライセンス

[MIT](LICENSE)
