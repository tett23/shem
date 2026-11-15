# ADR 0032: adr-guard を v0.13.1 に上げ、ADR のステータスの行を新形式に移す

Status: proposed

## 文脈

shem は、コミット済みの ADR がステータスの行とリンクを除いて変わらないことを、adr-guard で検査している。
検査は、git の pre-commit の hook（`exec adr-guard pre-commit --allow-status --allow-links`）と CI の `adr` ジョブで行う。
[ADR 0017](0017-update-adr-guard-to-v0.10.0.md) で adr-guard を v0.10.0 に上げ、[ADR 0024](0024-provenance-and-pinning-for-ci-and-releases.md) で、CI の action を `tett23/adr-guard@624df60fe8956ef52b3a275198ee938175324496 # v0.10.0` とコミットの SHA で固定した。
README.md、CONTRIBUTING.md と、その翻訳の README.ja.md、CONTRIBUTING.ja.md は、v0.10.0 を入れる手順を書いている。

adr-guard は、その後に v0.11.0、v0.12.0、v0.13.0、v0.13.1 を出した。

- v0.11.0 は、Codex の PreToolUse hook `adr-guard codex-hook` を足した（[tett23/adr-guard の ADR 0021](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0021-codex-hook.md)）。
- v0.12.0 は、ステータスの行の新形式と、状態の遷移の検査を足した（[tett23/adr-guard の ADR 0022](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0022-status-transitions.md)）。修正の連なりをたどる `adr-guard trace` も足した（[tett23/adr-guard の ADR 0023](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0023-trace-amendments.md)）。
- v0.13.0 は、移行のスクリプトに `--dry-run` を足し、移せない ADR を全件数えるようにした（[tett23/adr-guard の ADR 0024](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0024-migration-dry-run-and-error-count.md)）。新形式に `superseded-pending`、変更の種類 `kind`（`replace`、`supplement`、`note`）と注記 `note`、`https://` の修正先を足した（[tett23/adr-guard の ADR 0025](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0025-extend-canonical-status-amendments.md)）。旧形式の参照の読み方も広げた（[tett23/adr-guard の ADR 0026](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0026-read-more-legacy-references.md)、[tett23/adr-guard の ADR 0027](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0027-read-legacy-replaced-status.md)）。
- v0.13.1 は、旧形式の部分の名前から、`ADR NNNN` の直前の修飾子、先頭の `。`、末尾の `→` を除くようにした。参照の後ろに部分の名前の続きがある行と、括弧が部分の名前をまたぐ行は、移行スクリプトが「部分名を決められません」と報告し、移せない ADR として数える（[tett23/adr-guard の ADR 0028](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0028-fix-legacy-part-names.md)）。

新形式のステータスの行は `Status: <トークン>` である。
トークンは `proposed`、`accepted`、`accepted-pending`、`superseded-pending`、`rejected`、`discarded`、`deprecated`、または `superseded [<変更の JSON 配列>]` のいずれかとする。
変更の要素は `{"part":<部分の名前か null>,"by":[<リポジトリのルートからのパス>]}` である。

[tett23/adr-guard の ADR 0022](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0022-status-transitions.md) によれば、v0.12.0 からは、追加した ADR と、ステータスの行を変えた ADR に新形式が要る。
pre-commit の hook と `check-range` は状態の遷移も検査し、遷移の検査は `--allow-status`、`ADR_GUARD_ALLOW`、トレーラーでは免除されない。
旧形式の行は、変えないかぎりそのまま読める。

このため、v0.13.1 では、shem の ADR の規則が変わる。
CLAUDE.md、CONTRIBUTING.md、CONTRIBUTING.ja.md は、ADR のステータスの行が `ステータス:` で始まると定め、ADR を破棄するときはステータスを `破棄` にすると定めている。
v0.13.1 では、新しい ADR にもステータスを変えた ADR にも、この行をコミットできない。

## 調査した事項

この節の確認は、手元の adr-guard 0.13.1（`adr-guard -V` が `adr-guard 0.13.1` を出す）と、adr-guard の v0.13.1 のソースの `scripts/migrate-adr-status`（以下、移行スクリプト。リリースのアーカイブには入らない）で行った。

action の入力は v0.10.0 と v0.13.1 で同じである。
tett23/adr-guard のクローンで `git diff --quiet v0.10.0 v0.13.1 -- action.yml` が終了コード 0 で終わった。
タグ `v0.13.1` はコミット `ab55769152ed0f8a3465ee3435a2df43ab9d067f` を指す（同じクローンで `git rev-parse v0.13.1^{commit}`）。
このコミットの `Cargo.toml` の版は `0.13.1` で、action は `version` の入力を省くとこの版のリリースを取得する。

このブランチの作業ツリーで、`adr-guard links`、`adr-guard check-status`、`adr-guard check-numbers` は、どれも終了コード 0 で終わった。
`adr-guard links --fix --include-committed` は、どのファイルも書きかえなかった。

v0.13.1 の pre-commit の hook は、このリポジトリの写しで次のように振る舞った。

| ステージした変更 | 結果 |
| --- | --- |
| `ステータス: 提案` の新しい ADR | 拒否（「新形式のステータス行は `Status: ` で始めてください」） |
| `Status: accepted` の新しい ADR | 拒否（「許可されていない状態遷移: Absent -> accepted」） |
| `Status: proposed` の新しい ADR | 通る |

[ADR 0017](0017-update-adr-guard-to-v0.10.0.md) は、`ステータス: 採択` の行で最初にコミットした（`git show c2b7543:docs/adr/0017-update-adr-guard-to-v0.10.0.md` の 3 行目）。
v0.13.1 では、この手順は拒否される。

移行スクリプトの `--dry-run` は、移せる ADR を 31 件、移せない ADR を 0 件と数え、終了コード 0 で終わった（このほかに、`Status: proposed` のこの ADR を移行済みの 1 件と数えた）。
「部分名を決められません」と報告された ADR はなかった。
`採択→X は[ADR NNNN](…)` の形の 6 件は、`X` を部分の名前とする `superseded` になる。
たとえば [ADR 0007](0007-png-output-resolution-and-cli.md) の行は、`Status: superseded [{"by":["docs/adr/0009-render-pages-in-parallel.md"],"part":"並列の描画"},{"by":["docs/adr/0010-diff-images.md"],"part":"差分の画像"}]` になる。
`却下` の 1 件は `rejected` に、`採択` の 24 件は `accepted` になる。
v0.13.0 の移行スクリプトと比べて、出す新しい行は変わらなかった。

移行の全体は、このリポジトリの写しで確かめた。
写しには、この ADR の草稿を置いてコミットした。
`--write` を実行し、[ADR 0017](0017-update-adr-guard-to-v0.10.0.md) と [ADR 0024](0024-provenance-and-pinning-for-ci-and-releases.md) の行を下の「実装すること」のとおりに直した。
その後の `--dry-run` は「移行できる: 0件、移行済み: 32件、移行できない: 0件」を出した。
変更をコミットすると pre-commit の hook が通り、`adr-guard check-range --allow-status --allow-links <元の HEAD> HEAD`、`check-status`、`links`、`check-numbers` は、どれも終了コード 0 で終わった。
移行の後の 32 件は、`accepted` が 22 件、`superseded` が 8 件、`rejected` が 1 件、`proposed` が 1 件だった。
`adr-guard trace docs/adr/0017-update-adr-guard-to-v0.10.0.md` は、この ADR への `adr-guard の版` の置換の辺を出した。

## 実装すること

### CI と hook

- CI の `adr` ジョブの action を `tett23/adr-guard@ab55769152ed0f8a3465ee3435a2df43ab9d067f # v0.13.1` にする。ほかの入力は変えない。`check-numbers` は既定のまま実行する。
- `.githooks/pre-commit` のコマンドは変えない。

### ステータスの行の移行

この ADR を `Status: proposed` でコミットした後に、次の順で移す。

1. リポジトリのルートで、adr-guard の v0.13.1 のソースの `scripts/migrate-adr-status` を `--dry-run` で実行し、移せない ADR が 0 件であることを確かめてから、`--write` で書く。移せない ADR が出たときは、そのステータスの行を手で新形式に書く。
2. [ADR 0017](0017-update-adr-guard-to-v0.10.0.md) のステータスを `Status: superseded [{"by":["docs/adr/0032-update-adr-guard-to-v0.13.1.md"],"part":"adr-guard の版"}]` にする。
3. [ADR 0024](0024-provenance-and-pinning-for-ci-and-releases.md) のステータスを `Status: superseded [{"by":["docs/adr/0032-update-adr-guard-to-v0.13.1.md"],"part":"adr-guard の action を固定するコミット"}]` にする。

[ADR 0024](0024-provenance-and-pinning-for-ci-and-releases.md) は、action を `624df60` に固定すると定めている。
この ADR は、固定先のコミットだけを変え、コミットの SHA で固定するという決定は変えない。

### 文書

- README.md、CONTRIBUTING.md と、その翻訳の README.ja.md、CONTRIBUTING.ja.md の、adr-guard を入れる手順の版を v0.13.1 にする。英語版と翻訳は同じコミットで変える。
- CONTRIBUTING.md と CONTRIBUTING.ja.md の ADR の節で、ステータスの行の規則を新形式に改める。新しい ADR は `Status: proposed` でコミットすること、破棄するときのステータスを `discarded` にすること、決定を置き換えたときの元の ADR の行の書き方（`superseded [...]`）を書く。
- CLAUDE.md の docs/adr/ の項の「行頭が `ステータス:` のステータスの行をちょうど一つ持つ」を、新形式の `Status:` の行に改める。
- docs/specifications.md の「ADR の検査」の節の版を v0.13.1 にし、この ADR へのリンクを足す。ステータスの行の新形式と、pre-commit の hook と CI が状態の遷移を検査することを書く。
- `adr-guard links --fix --include-committed` を実行し、直すべき参照があれば直す。

## 実装しないこと

- Codex の hook（`adr-guard codex-hook`）は足さない。shem は Codex の hook の設定を持たないため。
- action をタグで参照する形には戻さない。[ADR 0024](0024-provenance-and-pinning-for-ci-and-releases.md) のとおり、コミットの SHA で固定する。
- 既存の ADR の本文は変えない。変えるのはステータスの行だけとする。
- docs/audit/ の記録の v0.10.0 の記述は変えない。記録はコミットの後に変えないため。
- 移行スクリプトをこのリポジトリに取り込まない。移行は一度だけの作業のため。

## 不変条件

なし。設定の値と、一度だけ行う移行の手順を決める ADR で、shem の描画の振る舞いを変えないため。

## テスト設計

設定の値とステータスの行の変更なので、自動のテストは足さない。次のように確かめる。

- `ruby -ryaml -e 'YAML.load_file(ARGV[0])' .github/workflows/ci.yml` で、CI の YAML が読めること。
- 移行の後に、移行スクリプトの `--dry-run` が「移行できない: 0件」を出し、`grep -l '^ステータス:' docs/adr/*.md` が何も出さないこと。
- 手元の adr-guard 0.13.1 で、`adr-guard links`、`adr-guard check-status`、`adr-guard check-numbers` が終了コード 0 で終わること。移行の変更をステージした状態で `.githooks/pre-commit` が通ること。
- コミットの後に、`adr-guard check-range origin/main HEAD --allow-status --allow-links` が終了コード 0 で終わること。
- `adr-guard trace docs/adr/0017-update-adr-guard-to-v0.10.0.md` が、この ADR への辺を出すこと。
- pull request の CI で、adr ジョブ（v0.13.1 の action）と translations ジョブが通ること。

## トレードオフ

- 新しい ADR は、まず `Status: proposed` でコミットし、採択は別のコミットでステータスの行を変えて記録することになる。[ADR 0017](0017-update-adr-guard-to-v0.10.0.md) のように最初から `ステータス: 採択` でコミットする手順は使えなくなる。そのかわり、採択や却下が状態の遷移として検査される。
- ステータスの行が英語のトークンと JSON になり、日本語の `採択→X は[ADR NNNN](…)` より人が読みにくくなる。修正の連なりは `adr-guard trace` でたどれる。
- 手元の adr-guard が v0.12.0 より古いと、新形式と状態の遷移の検査が手元では効かず、CI で初めて失敗に気づく。README と CONTRIBUTING の手順の版を v0.13.1 に上げて補う。

## 参考文献

- adr-guard v0.13.1 のリリース https://github.com/tett23/adr-guard/releases/tag/v0.13.1
- adr-guard v0.13.0 のリリース https://github.com/tett23/adr-guard/releases/tag/v0.13.0
- adr-guard v0.12.0 のリリース https://github.com/tett23/adr-guard/releases/tag/v0.12.0
- adr-guard v0.11.0 のリリース https://github.com/tett23/adr-guard/releases/tag/v0.11.0
- [adr-guard v0.13.1 の action.yml](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/action.yml)
- [tett23/adr-guard の ADR 0021](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0021-codex-hook.md)
- [tett23/adr-guard の ADR 0022](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0022-status-transitions.md)
- [tett23/adr-guard の ADR 0023](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0023-trace-amendments.md)
- [tett23/adr-guard の ADR 0024](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0024-migration-dry-run-and-error-count.md)
- [tett23/adr-guard の ADR 0025](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0025-extend-canonical-status-amendments.md)
- [tett23/adr-guard の ADR 0026](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0026-read-more-legacy-references.md)
- [tett23/adr-guard の ADR 0027](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0027-read-legacy-replaced-status.md)
- [tett23/adr-guard の ADR 0028](https://github.com/tett23/adr-guard/blob/ab55769152ed0f8a3465ee3435a2df43ab9d067f/docs/adr/0028-fix-legacy-part-names.md)
