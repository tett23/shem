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
- **少ない依存**：必要な部品（Flate、CFF と TrueType のフォントのアウトライン、PNG、JPEG）は、このリポジトリの中で書く。

shem は変化を見つけるものであり、描いた結果が正しいことを示すものではない。
同じ書き手が作ったプログラムとテスト用のラスタライザは、仕様の同じ読み違いを共有しうる。
正しさは、pdftoppm や PDFium など、独立したビューアーで確かめる。

## 版ごとの範囲

- **v0.1**：今の Golem が書く PDF の部分集合だけを読む。画像（Flate の標本と JPEG、SMask つき）と図版（Form XObject）は復号して描く。ほかの PDF は、多くの場合エラーで拒む。
- **v0.x**：Golem が書くものに追従し、v1 に向けて範囲を少しずつ広げる。
- **v1**：汎用の PDF のラスタライザを目指す。ほかの生成系が書く PDF も対象にする。

計画は [docs/roadmap.md](docs/roadmap.md) にある。

## 状態

v0.1 の範囲を実装した。
最新のリリース候補 v0.1.0-rc9 を [Releases](https://github.com/tett23/shem/releases) に置いている。
設計は [docs/adr/](docs/adr/) の ADR（Architecture Decision Record）で決めている。
仕様は [docs/specifications.md](docs/specifications.md) にある。

## インストール

[Releases](https://github.com/tett23/shem/releases) から、環境に合うアーカイブと `SHA256SUMS` を取得し、検証してから `shem` を `PATH` にあるディレクトリに置く。

| アーカイブ | 環境 |
|---|---|
| `shem-<タグ>-x86_64-unknown-linux-musl.tar.gz` | Linux の x86-64（静的にリンク） |
| `shem-<タグ>-aarch64-unknown-linux-musl.tar.gz` | Linux の aarch64（静的にリンク） |
| `shem-<タグ>-aarch64-apple-darwin.tar.gz` | Apple Silicon の macOS |
| `shem.wasm` | WebAssembly。ブラウザと Deno 用（どのリリースでも同じ名前） |

```sh
tag=v0.1.0-rc9
target=x86_64-unknown-linux-musl
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/shem-$tag-$target.tar.gz"
curl -fsSLO "https://github.com/tett23/shem/releases/download/$tag/SHA256SUMS"
grep -F " shem-$tag-$target.tar.gz" SHA256SUMS > check && sha256sum -c check
tar -xzf "shem-$tag-$target.tar.gz"
```

macOS では、`sha256sum -c check` の代わりに `shasum -a 256 -c check` を使う。
`SHA256SUMS` はアーカイブと同じリリースに置いているので、取得の破損は見つけられるが、ファイルを差し替えられたリリースは見つけられない。

v0.1.0-rc7 以降のリリースには、ビルドの出自の attestation が付いている。
[GitHub CLI](https://cli.github.com/) で、ファイルがこのリポジトリのリリースのワークフローでビルドされたことを確かめる。

```sh
gh attestation verify "shem-$tag-$target.tar.gz" -R tett23/shem
```

これらのリリースは immutable で、公開した後は、ファイルもタグも変えられない。
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

クレートのライブラリ（`shem::render::Session`、`shem::parallel::run`）は、shem が v0.x の間、版を上げずに変わりうる。

### ブラウザで使う

`shem.wasm` は、ブラウザ（Web Worker）か Deno の中で、ページを RGBA に描く。
import はなく、`WebAssembly.instantiate(module, {})` で読み込める。
関数は 32 ビットの整数（メモリの中のポインタ、長さ、番号）を受け取って返す。

| 関数 | 引数 | 戻り値 |
|---|---|---|
| `shem_alloc` | `len` | PDF を書き込む、0 で埋めた `len` バイトへのポインタ、または 0 |
| `shem_free` | `ptr` | なし。`shem_alloc` で得たものを解放する |
| `shem_open` | `ptr`、`len`、`dpi` | 文書の番号（1 から）、または 0 |
| `shem_page_count` | `doc` | ページの数。その文書がなければ 0 |
| `shem_render` | `doc`、`page` | 1 または 0。文書のページ `page`（0 から）を描く |
| `shem_image_ptr`、`shem_image_width`、`shem_image_height` | `doc` | 最後に描いたページの RGBA と、その大きさ |
| `shem_close` | `doc` | なし。文書とその画像を解放する |
| `shem_error_ptr`、`shem_error_len` | なし | 最後のエラーの UTF-8 の文 |

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

ここで `message` は、`shem_error_ptr()` から `shem_error_len()` バイトを読む。
`shem_alloc` は、そのバイト数を確保できなければ 0 を返す。そのときの文は `cannot allocate <len> bytes` である。
`shem_render` は描くページだけを検査するので、ほかのページを読む前に 1 枚目を表示できる。
エラーの文は、`shem render` が書く文と同じである。
画像は、その文書の次の `shem_render` か `shem_close` まで有効である。
パニックはトラップ（`WebAssembly.RuntimeError`）になる。そのときは新しいインスタンスを読み込む。

## 開発

[adr-guard](https://github.com/tett23/adr-guard) の v0.10.0 以降をインストールし、クローンごとに git の hook を有効にする。

```sh
cargo install --git https://github.com/tett23/adr-guard --tag v0.10.0
git config core.hooksPath .githooks
```

テストは `cargo test` で実行する。
`shem.wasm` を確かめるには、`rust-toolchain.toml` の版の Rust でビルドして、Deno のテストを実行する。

```sh
rustup toolchain install
rustup target add wasm32-unknown-unknown
sh scripts/ci/build-wasm.sh target/shem.wasm
SHEM_WASM=target/shem.wasm deno test --allow-read --allow-env wasm/test.ts
```

変更の進め方は [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) を参照。

## ライセンス

[MIT](LICENSE)。
JPEG の復号器は、Independent JPEG Group の libjpeg の整数の式を書き直して計算する。[NOTICE](NOTICE) を参照。
