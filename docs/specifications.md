# shem 仕様書

shem は、PDF のページを PNG の画像に描くラスタライザである。
最初の目的は、Golem（https://github.com/tett23/golem ）の PDF の出力の見た目の回帰テストで、見た目の変化を見つけることである。
描いた結果が正しいことを示すものではない。
v1 では、汎用の PDF のラスタライザを目指す（[roadmap.md](roadmap.md)）。

この文書は、shem の今の振る舞いを定める。
設計の決定は `docs/adr/` に連番の ADR として置く。

## 状態

まだ何も実装していない。
目的と範囲、Golem との取り決め、決定性の約束、描く範囲は、ADR で決めているところである。
版ごとの計画は [roadmap.md](roadmap.md) にある。

## 文書の翻訳

README.md と CONTRIBUTING.md は英語を正とし、README.ja.md と CONTRIBUTING.ja.md をその機械翻訳とする（[ADR 0008](adr/0008-english-canonical-readme-and-contributing.md)）。

- 英語版を変えるコミットは、同じコミットで対応する翻訳も変える。翻訳だけを変えるコミットは認める。
- コミットメッセージの最後の段落に `Translation-Unchanged: <理由>`（キーの大文字と小文字は区別しない。理由は空でない）があれば、翻訳を変えなくてよい。
- 検査は `scripts/check-translations.sh` が行う。git の commit-msg の hook はステージされた変更を、CI は範囲のマージ以外の各コミットを検査する。違反は `check-translations: <コミット>: <英語版> changed without <翻訳> (...)` を出し、終了コード 1 で終わる。使い方の誤りは終了コード 64 で終わる。

## 破棄した ADR

なし。
