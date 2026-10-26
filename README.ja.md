# shem

[![CI](https://github.com/tett23/shem/actions/workflows/ci.yml/badge.svg)](https://github.com/tett23/shem/actions/workflows/ci.yml)

shem は、開発中の PDF のラスタライザである。
PDF のページを PNG の画像に描き、どの機械でも同じバイト列を出すことを目指す。

[English README](README.md)

この文書は [README.md](README.md)（英語）の機械翻訳である。
英語版を正とし、内容が食い違うときは英語版に従う。

## 概要

shem は、PDF の出力の見た目の回帰テストのためのラスタライザとして始まる。
PDF の各ページを画像に描き、テストが二つのコミットの画像を比べて見た目の変化を見つけられるようにする予定である。
最初の利用者は、組版システムの [Golem](https://github.com/tett23/golem) である。

目標は次のとおりである。

- **決定性**：同じ入力から、x86-64 でも aarch64 でも、Linux でも macOS でも、バイト単位で同じ PNG を出す。描画には整数の演算だけを使う。
- **読むものに厳格**：読まないものに出会うと、黙ってほかのものを描くのではなく、エラーで止まる。
- **少ない依存**：必要な部品（Flate、CFF と TrueType のフォントのアウトライン、PNG）は、このリポジトリの中で書く。

shem は変化を見つけるものであり、描いた結果が正しいことを示すものではない。
同じ書き手が作ったプログラムとテスト用のラスタライザは、仕様の同じ読み違いを共有しうる。
正しさは、pdftoppm や PDFium など、独立したビューアーで確かめる。

## 版ごとの範囲

- **v0.1**：今の Golem が書く PDF の部分集合だけを読む。画像と図版は復号せず、バツ印の付いた灰色の箱で描く。ほかの PDF は、多くの場合エラーで拒む。
- **v0.x**：Golem が書くものに追従し、v1 に向けて範囲を少しずつ広げる。
- **v1**：汎用の PDF のラスタライザを目指す。ほかの生成系が書く PDF も対象にする。

計画は [docs/roadmap.md](docs/roadmap.md) にある。

## 状態

v0.1 の範囲を実装した。
最初のリリース候補 v0.1.0-rc1 を [Releases](https://github.com/tett23/shem/releases) に置いている。
設計は [docs/adr/](docs/adr/) の ADR（Architecture Decision Record）で決めている。
仕様は [docs/specifications.md](docs/specifications.md) にある。

## インストール

[Releases](https://github.com/tett23/shem/releases) から、環境に合うアーカイブと `SHA256SUMS` を取得し、検証してから `shem` を `PATH` にあるディレクトリに置く。

| アーカイブ | 環境 |
|---|---|
| `shem-<タグ>-x86_64-unknown-linux-musl.tar.gz` | Linux の x86-64（静的にリンク） |
| `shem-<タグ>-aarch64-unknown-linux-musl.tar.gz` | Linux の aarch64（静的にリンク） |
| `shem-<タグ>-aarch64-apple-darwin.tar.gz` | Apple Silicon の macOS |

```sh
tag=v0.1.0-rc1
target=x86_64-unknown-linux-musl
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/shem-$tag-$target.tar.gz"
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/SHA256SUMS"
sha256sum --ignore-missing -c SHA256SUMS
tar -xzf "shem-$tag-$target.tar.gz"
```

macOS では `shasum -a 256 --ignore-missing -c SHA256SUMS` を使う。
macOS のバイナリは署名と公証をしていない。`curl` や `gh` で取得したアーカイブには検疫の属性が付かない。

ソースからビルドする場合は、Cargo を使う。

```sh
cargo build --release
```

## 使い方

PDF のすべてのページを、ディレクトリの PNG のファイルに描く。

```sh
shem render --dpi 150 input.pdf out
```

これは `out/page-0001.png`、`out/page-0002.png` などを書く。
`--format ppm` を付けると、PNG の代わりに PPM のファイルを書く。PPM のバイト列は画素だけで決まる。
`--pages 2-5` を付けると、一部のページだけを描く。
ページは、すべてのコアで並列に描く。`--jobs <N>` で同時に描く数を決められる。出力はこの値によらない。
描くページのどれかが shem の読む部分集合の外にあれば、shem はファイルを一つも書かず、その場所を標準エラーに書き、終了コード 1 で終わる。
すべてのオプションと終了コードは `shem --help` で読める。

二つの描画の違う場所を見るには、画像を比べる。

```sh
shem diff before/page-0003.png after/page-0003.png diff.png
shem diff before after diffs
```

一つ目は二つのファイルを比べる。二つ目は、二つのディレクトリの画像を名前で対応させて比べ、違う組ごとに差分の画像を書き、違うページ、増えたページ、減ったページを一覧する。
差分の画像では、後の画像のほうが暗い画素が赤、明るい画素が青、片方にしかない画素が緑になり、違う場所を橙の枠が囲む。
終了コードは、違いがなければ 0、違いがあれば 1、エラーなら 2 である。

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
