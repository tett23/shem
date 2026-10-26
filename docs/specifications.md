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

## 読む PDF の部分集合

v0.1 は、今の Golem の Writer が書く PDF の部分集合を読む（[ADR 0002](adr/0002-pdf-subset-and-out-of-scope-input.md)）。
範囲の外の入力と壊れた入力は、エラーで止める。読み飛ばしも、近いものでの代用もしない。
エラーは `page <番号>`、`object <番号>`、`offset <バイトの位置>` のうちわかるものと内容を、`page 3: offset 120: unsupported operator TJ` の形で示す。

### ファイルの構造

- 先頭は `%PDF-` で始まる。末尾の 1024 バイトにある `startxref` から、古典的な xref の表と trailer を読む。
- xref のストリーム、オブジェクトのストリーム（`/Type /ObjStm`）、trailer の `Prev`、`XRefStm`、`Encrypt`、世代番号が 0 でないオブジェクトは、エラーにする。xref の表の位置にそのオブジェクトがないときもエラーにする。
- オブジェクトは使うときに読む。ストリームの `Length` は直接の整数か、整数への間接参照とする。
- 中身を読むストリームのフィルタは、なしか `FlateDecode` だけとする。`DecodeParms` はエラーにする。Flate の伸長は自前で行い、Adler-32 が合わなければエラーにする。
- ページの木をたどり、`Resources`、`MediaBox`、`Rotate` を継承する。木の循環はエラーにする。`Rotate` は 0 だけを認め、`UserUnit` はエラーにする。`Contents` の配列は、ストリームの間に空白を一つ挟んでつなぐ。注釈は読まない。

### ページの内容の演算子

解釈する演算子は `q Q cm w m c h re f k K cs scn BT ET Tf Tz Tm Tr Tj Do BDC EMC` だけで、ほかの演算子は `unsupported operator <演算子>` のエラーにする。

- 被演算子の数と型が合わなければ `wrong operands of <演算子>` のエラーにする。`w` の負の値もエラーにする。`Tr` は 0 と 2 だけを認める。
- `q` と `Q`、`BT` と `ET`、`BDC` と `EMC` は対応する。文字のオブジェクトの中では `q Q cm Do BT` とパスの演算子を、パスの途中では `m c h re f` 以外を、エラーにする（ISO 32000-1 の附属書 L の図 9）。`c` と `h` は現在の点を要る。
- `Tf`、`cs`、`Do` の名前は、ページの `Resources` の `Font`、`ColorSpace`、`XObject` になければエラーにする。

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
