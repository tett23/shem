# ADR 0008: README と CONTRIBUTING は英語を正とし、日本語版を同じコミットで更新する

ステータス: 採択

## 文脈

shem の v0.1 は今の Golem の出力に合わせて作るが、v1 では汎用の PDF のラスタライザを目指し、Golem 以外の利用者も対象にする（[ADR 0001](0001-scope-golem-contract-and-determinism.md)）。
Golem を使わない利用者と貢献者が最初に読む README.md と CONTRIBUTING.md は、英語で書くほうが届く範囲が広い。
一方で、メンテナーと ADR、仕様書は日本語で書く。

メンテナーは、README と CONTRIBUTING は英語を正とし、日本語版は機械翻訳して同時に更新すると決めた（2026-10-26）。
リポジトリの最初のコミットで、README.md と CONTRIBUTING.md を英語で、README.ja.md と CONTRIBUTING.ja.md をその翻訳として置き、CLAUDE.md と CONTRIBUTING に規則を書いた。
しかし、規則を書いただけでは、英語版だけを変えたコミットを止められない。
翻訳が遅れると、日本語版を読む人は古い手順に従うことになる。

peg-hs も英語と日本語の README を持つが、同期を検査する仕組みはない（日本語を正としている）。

## 実装すること

### 対象

- 英語版と翻訳の組は、次の二つとする。
  - `README.md` と `README.ja.md`
  - `CONTRIBUTING.md` と `CONTRIBUTING.ja.md`
- 翻訳の冒頭に、英語版の機械翻訳であること、英語版を正とし食い違うときは英語版に従うことを書く。

### 翻訳の作り方

- 英語版を変えたら、変えた節を機械翻訳し、翻訳の対応する節を置き換える。翻訳は、変更を作る人（Claude Code を含む）が、コミットの前に行う。
- 翻訳の文体は docs/ の日本語の文書に合わせる（一文一行、である調）。
- CI の中で翻訳を作ることはしない（下の「実装しないこと」）。

### 検査

- 英語版を変えたコミットは、同じコミットで対応する翻訳も変える。これを `scripts/check-translations.sh` で検査する。
- 例外として、コミットメッセージの最後の段落に `Translation-Unchanged: <理由>` のトレーラーがあれば、翻訳を変えなくてよい。英語版のリンクの修正や、英語の綴りの誤りの修正など、訳に影響しない変更のためである。理由が空なら認めない。
- 翻訳だけを変えるコミットは認める。訳の誤りを直すためである。
- 検査は二か所で行う。
  - git の `commit-msg` の hook（`.githooks/commit-msg`）：ステージされた変更とコミットメッセージを検査する。トレーラーを読むため、`pre-commit` ではなく `commit-msg` で行う。
  - CI：プルリクエストと `main` への push で、範囲のマージ以外の各コミットを、親との差分とコミットメッセージで検査する。
- 違反のメッセージは `check-translations: <コミット>: <英語版> changed without <翻訳> (add Translation-Unchanged: <reason> if the translation is not affected)` とし、終了コードを 1 にする。
- スクリプトは POSIX の sh で書き、macOS の BSD 版の道具でも Linux の GNU 版の道具でも動くようにする。

## 実装しないこと

- CI での自動の機械翻訳（API の鍵を CI に置くこと、翻訳のプルリクエストを自動で作ることを含む）。
- 翻訳の内容が英語版と合っているかの検査。検査するのは、同じコミットで変えたかどうかだけである。
- ADR、docs/specifications.md、CLAUDE.md の翻訳。
- 英語版と翻訳の節の数や見出しの対応の検査。

## テスト設計

`scripts/test-check-translations.sh` に、一時的な git のリポジトリを作って確かめるテストを書き、CI で実行する。

- README.md だけを変えたコミット：違反になり、終了コードが 1 で、メッセージに README.ja.md が入る。
- README.md と README.ja.md を変えたコミット：通る。
- README.ja.md だけを変えたコミット：通る。
- CONTRIBUTING.md だけを変え、`Translation-Unchanged: fix a link` のトレーラーを付けたコミット：通る。
- トレーラーの理由が空のコミット、トレーラーが最後の段落にないコミット：違反になる。
- README.md と CONTRIBUTING.ja.md を変えたコミット：README.md の違反になる（組が違う）。
- 範囲の検査：違反のないコミットと違反のあるコミットが並ぶ範囲で、違反のあるコミットだけが報告される。マージのコミットは検査しない。
- `commit-msg` の hook：違反のあるコミットが作られず、違反のないコミットは作られる。
- テストを macOS（BSD 版の道具）と Linux で実行する。

## トレードオフ

- 同じコミットで変えたかだけを見るので、英語版の別の節を訳さずに、翻訳の一行だけを変えても通る。内容の対応はレビューで確かめる。
- CI で自動に翻訳しないので、翻訳の手間は変更を作る人にかかる。そのかわり、CI に翻訳の API の鍵を置かずに済み、翻訳の文面をレビューできる。
- `commit-msg` の hook は `git commit --no-verify` で迂回できる。CI の検査で見つける。
- トレーラーによる例外を設けたので、翻訳を怠るのにも使える。理由が履歴に残るので、レビューで確かめる。

## 参考文献

- adr-guard の `ADR-Guard-Allow` のトレーラーの設計（トレーラーで例外を認める同じ形）https://github.com/tett23/adr-guard/blob/a54a2c6c3f6976f56b7614615eadce25cf84d382/docs/adr/0008-allow-intentional-adr-rewrites.md
- git-interpret-trailers(1) https://git-scm.com/docs/git-interpret-trailers
- githooks(5) の commit-msg https://git-scm.com/docs/githooks#_commit_msg
