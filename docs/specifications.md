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

## 決定性

同じ入力と同じオプションから、x86-64 と aarch64、Linux と macOS で、バイト単位で同じ出力を出す（[ADR 0001](adr/0001-scope-golem-contract-and-determinism.md)）。

- 入力から画素までの経路で、浮動小数点数を使わない。CI は `src/` に `f32` と `f64` がないことを `scripts/ci/check-no-float.sh` で確かめる。
- 実数と行列の要素は Q32.32（符号付き 64 ビットの整数、小数部 32 ビット）で表す。
- 十進の数の文字列は、桁数によらず正確に、最も近い Q32.32 の値に丸める（同じ近さなら偶数）。整数部の絶対値は 2^31 未満とし、超えればエラーにする。
- 掛け算、割り算、行列の積と点の変換は、128 ビットの整数で計算してから一度だけ丸める（最近接の偶数への丸め）。平方根は切り捨てる。
- 桁あふれはエラーにする。黙って折り返さない。

## 配布

`v` で始まるタグを push すると、Linux の x86-64 と aarch64（musl で静的にリンク）と macOS の aarch64 のアーカイブ `shem-<タグ>-<ターゲット>.tar.gz` と `SHA256SUMS` を、GitHub の Releases に置く（[ADR 0001](adr/0001-scope-golem-contract-and-determinism.md)）。
タグと `Cargo.toml` の版が合わなければ、リリースは失敗する。

## 文書の翻訳

README.md と CONTRIBUTING.md は英語を正とし、README.ja.md と CONTRIBUTING.ja.md をその機械翻訳とする（[ADR 0008](adr/0008-english-canonical-readme-and-contributing.md)）。

- 英語版を変えるコミットは、同じコミットで対応する翻訳も変える。翻訳だけを変えるコミットは認める。
- コミットメッセージの最後の段落に `Translation-Unchanged: <理由>`（キーの大文字と小文字は区別しない。理由は空でない）があれば、翻訳を変えなくてよい。
- 検査は `scripts/check-translations.sh` が行う。git の commit-msg の hook はステージされた変更を、CI は範囲のマージ以外の各コミットを検査する。違反は `check-translations: <コミット>: <英語版> changed without <翻訳> (...)` を出し、終了コード 1 で終わる。使い方の誤りは終了コード 64 で終わる。

## 破棄した ADR

なし。
