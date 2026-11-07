# ADR 0017: adr-guard を v0.10.0 に上げる

ステータス: 採択

## 文脈

shem は、リポジトリの最初のコミットから adr-guard を使い、コミット済みの ADR がステータスの行とリンクを除いて変わらないことを、pre-commit の hook と CI で検査している。
導入は ADR を書かずに行い、CI の action と README、CONTRIBUTING の手順は v0.8.3 を指している。

adr-guard は、その後に v0.9.0、v0.9.1、v0.10.0 を出した。

- v0.9.0 は、同じディレクトリで番号が重複した ADR を報告する `adr-guard check-numbers` を足した。action は既定でこれを実行し、重複があれば CI が失敗する。重複を直す `adr-guard renumber` と、番号の付け替えを認める `--allow-renumber` も足した（[tett23/adr-guard の ADR 0018](https://github.com/tett23/adr-guard/blob/main/docs/adr/0018-repair-duplicate-adr-numbers.md)）。
- v0.9.1 は、`renumber` の誤報を直した。
- v0.10.0 は、`ADR NNNN` の直前の語を修飾子として読み、ほかのディレクトリやモジュールの ADR への参照を扱う（[tett23/adr-guard の ADR 0019](https://github.com/tett23/adr-guard/blob/main/docs/adr/0019-module-qualified-adr-references.md)）。`links` と pre-commit は、対象の無い修飾子付きの参照、修飾子と食い違うリンク、リンクの外に出た修飾子を報告する。シンボリックリンクの `.md` は検査しない（[tett23/adr-guard の ADR 0020](https://github.com/tett23/adr-guard/blob/main/docs/adr/0020-skip-symlinked-documents.md)）。

v0.10.0 では、既存のリポジトリでリンクの検査が失敗しうる。
手元で v0.10.0 の `adr-guard links --fix --include-committed` を実行したところ、shem では書きかわるファイルはなく、`links`、`check-status`、`check-numbers` はどれも終了コード 0 で終わった。
shem はモジュールごとの ADR を持たず、ADR は `docs/adr/` の一つのディレクトリにある。

## 実装すること

- CI の adr ジョブの action を `tett23/adr-guard@v0.10.0` にする。ほかの入力は変えない。`check-numbers` は既定のまま実行する。
- README.md、CONTRIBUTING.md と、その翻訳の README.ja.md、CONTRIBUTING.ja.md の、adr-guard を入れる手順の版を v0.10.0 にする。
- CONTRIBUTING.md と CONTRIBUTING.ja.md に、CI が ADR の番号の重複も検査することを書く。
- docs/specifications.md に、ADR の検査の節を足す。
- `.githooks/pre-commit` のコマンドは変えない。

## 実装しないこと

- モジュールごとの ADR のディレクトリは作らない。修飾子付きの参照の規則は、そうした ADR を持つまで文書に書かない。
- `--allow-renumber` は使わない。番号が重複したときは、その都度直す。
- 既存の ADR の本文は変えない。

## テスト設計

設定の値の変更なので、自動のテストは足さない。次のように確かめる。

- `ruby -ryaml -e 'YAML.load_file(ARGV[0])' .github/workflows/ci.yml` で、CI の YAML が読めること。
- 手元の adr-guard 0.10.0 で、`adr-guard links`、`adr-guard check-status`、`adr-guard check-numbers` が終了コード 0 で終わること。
- コミットの後に、`adr-guard check-range origin/main HEAD --allow-status --allow-links` が終了コード 0 で終わること。
- pull request の CI で、adr ジョブ（v0.10.0 の action）と translations ジョブが通ること。

## トレードオフ

- 番号の重複が CI で止まるので、並行して書いた ADR が同じ番号を取ったとき、後からマージするほうが番号を付け替える手間を負う。そのかわり、`main` に同じ番号の ADR が二つ並ぶことはなくなる。
- 修飾子の規則により、`ADR NNNN` の直前に `/` や `-` を含む語を置くと、修飾子として読まれる。ほかのリポジトリの ADR を指すときは、外部リンクの中に入れて書く必要がある。

## 参考文献

- adr-guard v0.10.0 のリリース https://github.com/tett23/adr-guard/releases/tag/v0.10.0
- adr-guard v0.9.1 のリリース https://github.com/tett23/adr-guard/releases/tag/v0.9.1
- adr-guard v0.9.0 のリリース https://github.com/tett23/adr-guard/releases/tag/v0.9.0
- [tett23/adr-guard の ADR 0018](https://github.com/tett23/adr-guard/blob/main/docs/adr/0018-repair-duplicate-adr-numbers.md)
- [tett23/adr-guard の ADR 0019](https://github.com/tett23/adr-guard/blob/main/docs/adr/0019-module-qualified-adr-references.md)
- [tett23/adr-guard の ADR 0020](https://github.com/tett23/adr-guard/blob/main/docs/adr/0020-skip-symlinked-documents.md)
