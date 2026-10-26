# shem への貢献

[English](CONTRIBUTING.md)

この文書は [CONTRIBUTING.md](CONTRIBUTING.md)（英語）の機械翻訳である。
英語版を正とし、内容が食い違うときは英語版に従う。

## 開発環境

Rust の stable のツールチェーンを使う。
[adr-guard](https://github.com/tett23/adr-guard) の v0.8.3 以降をインストールし、クローンしたら git の hook を有効にする。
この設定はリポジトリに保存されないため、クローンのたびに実行する。

```sh
cargo install --git https://github.com/tett23/adr-guard --tag v0.8.3
git config core.hooksPath .githooks
```

CI は、Linux（x86-64 と aarch64）と macOS（aarch64）で次の検査を実行する。

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

## 変更の流れ

1. ADR を書き、レビューを受ける。レビューの前に ADR をコミットしない。
2. ADR のテスト設計にあるテストを書き、失敗することを確かめる。
3. 変更を実装し、テストを通す。
4. 同じプルリクエストで docs/specifications.md と README を更新する。
5. ブランチを push し、プルリクエストを作る。
6. CI が通ったら、マージコミット（`--no-ff`）で `main` にマージする。

誤字の修正や、仕様を変えない文書の修正には ADR は要らない。

## ADR

- ADR は `docs/adr/` に置き、`0001-scope-and-determinism.md` のように、連番と、読んでわかる kebab-case の名前を付ける。
- ADR は、`ステータス:` で始まるステータスの行をちょうど一つ持つ。
- コミットした ADR は、ステータスの行とリンクを除いて変えない。adr-guard が pre-commit の hook と CI でこれを強制する。決定を変えるときは、古い ADR を置き換える新しい ADR を書く。
- ADR を破棄するときは、ステータスを `破棄` にし、理由を docs/specifications.md に書く。
- ADR からソースコードを参照するときは、コミットのハッシュを含む GitHub の URL で書く。`main` は rebase しないので、その URL は変わらない。
- ADR は、決定に使った文献を末尾に挙げる。
- ADR や文書を書いたら、`adr-guard links --fix` で ADR の参照にリンクを付ける。

## 言語

- コミットメッセージ、リリースノート、CHANGELOG は英語で書く。
- ADR と docs/specifications.md は日本語で書く。
- README.md と CONTRIBUTING.md は英語で書き、英語版を正とする。README.ja.md と CONTRIBUTING.ja.md はその機械翻訳である。英語版を変えるコミットは、同じコミットで翻訳も更新する。

## コード

- Rust では関数型のスタイルを優先する。
- ライセンスに問題がなく、ここで書けるものは、依存に加えない。ここで書き、本家と同じくらい厚いテストを付ける。
- どの機械でも同じ出力になるよう、描画には整数と固定小数点の演算だけを使う。入力から画素までの経路で浮動小数点数を使わない。
- flaky なテストを、通るまで再実行しない。原因を見つけて直す。

## Git

- `main` を rebase しない。ブランチは、マージするまでは rebase してよい。
- `--no-ff` でマージする。
- `v` で始まるタグはリリースである。`v0.1.0-rc1` のようなタグはプレリリースである。
- リリースノートは GitHub の Releases に書き、Added、Changed、Removed、Fixed に分ける。各変更にはプルリクエストと ADR を添え、ADR には絶対リンクを使う。
