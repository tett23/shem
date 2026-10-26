# ADR 0002: v0.1 が読む PDF の部分集合と、範囲の外の入力の扱い

ステータス: 採択

## 文脈

[ADR 0001](0001-scope-golem-contract-and-determinism.md) で、v0.1 が描くのは今の Golem の Writer が書く PDF の部分集合だと決めた。
この ADR は、v0.1 の部分集合の境界と、境界の外に出会ったときの振る舞いを決める。
v1 で汎用のラスタライザにするときは、別の ADR でこの境界を広げる。

Golem の Writer の出力には、次の性質がある（Golem の origin/main の 1254092a で調べた）。

- ファイルは、ヘッダー、番号の順に並んだオブジェクト、古典的な xref の表、trailer からなる（[Writer.hs:330-350](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L330-L350)）。xref のストリームもオブジェクトのストリームも書かず、世代番号は常に 0 で、ストリームの `Length` は直接の整数である。
- ページの木は、`Pages` の下に `Page` が並ぶ一段のものである。ページの辞書は `MediaBox`、`TrimBox`、`BleedBox`、`Resources`、一つの `Contents` を持ち、`Rotate` を書かない（[Writer.hs:418-435](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L418-L435)）。
- ページの内容のストリームは `FlateDecode` で圧縮する。
- ページの内容で使う演算子は `BT ET Tf Tz Tm Tj Tr w re f m c h q Q cm Do k K cs scn BDC EMC` である（[Writer.hs:228-289](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L228-L289)）。`TJ`、`Td`、`Tc`、`Tw`、`TL` は書かず、字ごとに `Tm` を置き、一つの `Tj` に一字だけを入れる。
- ページの `Resources` は `Font`、`XObject`、`ColorSpace` だけを持つ（[Writer.hs:403-415](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L403-L415)）。
- `Do` で呼ぶのは、画像と図版の Form だけである。SVG の図版の Form は `CS sc SC J j M d l W n f* B B* S` を加えて使い、取り込んだ PDF の図版の Form は利用者の PDF の演算子をそのまま写すので、集合が閉じていない。

v0.1 では、画像と Form の中身を描かない（[ADR 0006](0006-placeholder-boxes-for-images-and-forms.md)）。
したがって、v0.1 で解釈する内容のストリームは、ページの内容だけでよい。

範囲の外の入力を読み飛ばすと、Golem の Writer が新しい演算子を書き始めたときに、その描画が画像に現れない。
見た目の回帰テストはその変化を見逃し、テストが通ったまま描画の欠落が続く。

## 実装すること

### 範囲の外に出会ったときの振る舞い

- 範囲の外の入力、または壊れた入力に出会ったら、エラーで止まる。読み飛ばしも、近いものでの代用もしない。
- エラーは標準エラーに一行で書き、終了コードを 1 にする。形は `shem: <入力のパス>: <場所>: <内容>` とする。
  - 場所は、わかる範囲で `page <ページの番号>`、`object <番号>`、内容のストリームの中のバイトの位置（`offset <n>`）を並べる。
  - 範囲の外の演算子なら、内容に `unsupported operator <演算子>` を書く。範囲の外の資源やキーの値なら、`unsupported <キー> <値>` の形にする。
- 出力を一つも書かないうちに止まるよう、画素を描く前に、描く対象のすべてのページについて、ファイルの構造、資源、内容のストリームを検査する（[ADR 0007](0007-png-output-resolution-and-cli.md)）。

### ファイルの構造

- 先頭は `%PDF-` で始まる。版の数字は問わない。
- 末尾の `startxref` から古典的な xref の表を読み、trailer の辞書を読む。
- 次のものはエラーにする。
  - xref のストリーム、オブジェクトのストリーム（`/Type /ObjStm`）、trailer の `Prev`（増分更新）、`Encrypt`
  - 世代番号が 0 でないオブジェクトへの参照
  - xref の表の位置にオブジェクトがない、オブジェクトの番号が表と合わないなどの不整合
- オブジェクトは、真偽値、整数、実数、文字列（リテラルと 16 進）、名前、配列、辞書、ストリーム、null、間接参照を読む。名前の `#xx` の符号化と、リテラルの文字列のエスケープ（八進を含む）を解く。
- ストリームの `Length` は直接の整数か、整数への間接参照とする。
- オブジェクトは、使うときに読む。使わないオブジェクトの中身は検査しない。

### ストリームのフィルタ

- shem が中身を読むストリーム（ページの内容、フォントのプログラム）のフィルタは、なしか `FlateDecode` だけとする。`DecodeParms` があればエラーにする（予測子を扱わない）。
- `FlateDecode` の伸長（zlib の形式、RFC 1950 と RFC 1951）は自前で書く。Adler-32 の検査値が合わなければエラーにする。
- 中身を読まないストリーム（画像、Form）は、フィルタを問わない。

### ページ

- `Root` の `Pages` から `Kids` をたどり、ページを順に並べる。`Pages` の入れ子も読み、`Resources` と `MediaBox` の継承（ISO 32000-1 の 7.7.3.4）を扱う。木の循環はエラーにする。
- 描く範囲は `MediaBox` とする（[ADR 0007](0007-png-output-resolution-and-cli.md)）。`CropBox` などほかの箱は読まない。
- `Rotate` は、ないか 0 だけを認める。ほかの値はエラーにする。
- `UserUnit` があればエラーにする。
- `Contents` は一つのストリームか、ストリームの配列とする。配列なら、ストリームの間に空白を一つ挟んでつないだものを内容とする（ISO 32000-1 の 7.7.3.3）。
- 注釈（`Annots`）は描かない。注釈があってもエラーにしない。Golem は注釈を書かず、注釈は印刷の見た目に含まれないからである。

### ページの内容の演算子

ページの内容で、次の演算子だけを解釈する。
これ以外の演算子（`BI` のインライン画像、`gs`、`sh`、`W`、`S`、`TJ` など）はすべてエラーにする。

| 分類 | 演算子 | 被演算子 | 決める ADR |
|---|---|---|---|
| グラフィックスの状態 | `q` `Q` | なし | この ADR |
| | `cm` | 数 6 | この ADR |
| | `w` | 数 1（負ならエラー） | [ADR 0003](0003-paths-fill-stroke-and-antialiasing.md) |
| パス | `m` | 数 2 | [ADR 0003](0003-paths-fill-stroke-and-antialiasing.md) |
| | `c` | 数 6 | 同上 |
| | `h` | なし | 同上 |
| | `re` | 数 4 | 同上 |
| | `f` | なし | 同上 |
| 色 | `k` `K` | 数 4 | [ADR 0005](0005-device-colors-and-cmyk-conversion.md) |
| | `cs` | 名前 1 | 同上 |
| | `scn` | 数 1 | 同上 |
| 文字 | `BT` `ET` | なし | [ADR 0004](0004-glyph-outlines-cid-mapping-and-vertical-writing.md) |
| | `Tf` | 名前 1、数 1 | 同上 |
| | `Tz` | 数 1 | 同上 |
| | `Tm` | 数 6 | 同上 |
| | `Tr` | 整数 1（0 と 2 だけ） | 同上 |
| | `Tj` | 文字列 1 | 同上 |
| XObject | `Do` | 名前 1 | [ADR 0006](0006-placeholder-boxes-for-images-and-forms.md) |
| 印の付いた内容 | `BDC` | 名前 1、辞書か名前 1 | この ADR |
| | `EMC` | なし | この ADR |

- 被演算子の数と型が表と合わなければエラーにする。数は整数でも実数でもよい。
- 字句は、空白、区切り文字、コメント（`%` から行末まで）を ISO 32000-1 の 7.2 のとおりに扱う。
- グラフィックスの状態のスタック：`Q` に対応する `q` がない、内容の末尾で `q` が閉じていない場合はエラーにする。
- 文字のオブジェクト：`BT` の入れ子、`BT` の外の `ET`、`Tm`、`Tj`、閉じていない `BT` はエラーにする。`BT` と `ET` の間では、`q`、`Q`、`cm`、パスの演算子、`Do` をエラーにする（ISO 32000-1 の附属書 L の図 9 で、文字のオブジェクトの中に置けない演算子）。
- パス：`m` か `re` の前の `c` と `h`、パスの途中での `f` 以外の演算子（パスの構築と塗りの間に置けないもの）はエラーにする。
- `BDC` と `EMC`：対応を検査し、描画には影響させない。`BDC` の被演算子の名前が資源の `Properties` を指していても、読まない。
- `Tf`、`cs`、`Do` の名前は、ページの `Resources` の `Font`、`ColorSpace`、`XObject` から引く。見つからなければエラーにする。
- `Resources` のうち、使わない資源の種類（`ExtGState`、`Pattern` など）があってもエラーにしない。使ったときだけ検査する。

## 実装しないこと

- 増分更新、xref のストリーム、オブジェクトのストリーム、暗号化、壊れたファイルの修復。
- `FlateDecode` 以外のフィルタと、予測子。
- SVG の図版の Form と、取り込んだ PDF の図版の Form の中身の解釈（[ADR 0006](0006-placeholder-boxes-for-images-and-forms.md) で箱にする）。図版を描くときに、別の ADR で範囲を広げる。
- 注釈、しおり、構造のツリー、印の付いた内容の意味（`ActualText` など）。
- 範囲の外を警告にして描き続けるオプション。

## テスト設計

### ファイルの構造

- 手で書いた最小の PDF（一ページ、内容が空）を読み、ページの数と `MediaBox` が取れる。
- xref のストリーム、`/Type /ObjStm`、`Prev`、`Encrypt`、世代番号 1 の参照、xref の表の位置のずれのそれぞれを含む PDF で、終了コードが 1 になり、エラーの文言が期待どおりである。
- `Pages` の入れ子と `Resources`、`MediaBox` の継承を読める。木が循環する PDF はエラーになる。
- `Rotate 90`、`UserUnit 2` はエラーになる。`Rotate 0` は通る。
- `Contents` の配列は、つないだものとして解釈される（二つ目のストリームの先頭の演算子が一つ目の末尾とつながらない）。

### Flate の伸長

- RFC 1951 の三種類のブロック（無圧縮、固定のハフマン、動的なハフマン）をそれぞれ含むデータを伸長できる。データは外の zlib で作ってバイナリの fixture として置き、作り方を `tests/fixtures/README.md` に書く。
- 長さと距離の境界の値（長さ 3 と 258、距離 1 と 32768）、窓をまたぐ参照、空のデータ。
- Adler-32 の不一致、不正なハフマンの符号、ブロックの途中の終わり、窓の外を指す距離は、エラーになる。
- Golem の出力の fixture のすべてのストリームを伸長できる。

### 内容のストリーム

- 表のすべての演算子について、正しい被演算子で通り、被演算子の数か型が違うとエラーになる。
- 表にない演算子（`TJ`、`Td`、`S`、`W`、`gs`、`sh`、`BI`、`l` など）でエラーになり、エラーに演算子の名前とページの番号とバイトの位置が入る。
- `q` と `Q` の不釣り合い、`BT` の入れ子、`BT` の外の `Tj`、`BT` の中の `q`、`BDC` と `EMC` の不釣り合いは、エラーになる。
- 資源にないフォント、色空間、XObject の名前は、エラーになる。
- 三ページの PDF の三ページ目にだけ範囲の外の演算子があるとき、出力のディレクトリにファイルが一つもできない。
- Golem の出力の fixture は、エラーなしに通る。

## トレードオフ

- 範囲を今の Golem の Writer の出力にぴったり合わせたので、v0.1 では、ほかの PDF の生成系の出力は多くの場合エラーになる。ほかの生成系の出力を描くのは、v1 に向けた範囲の拡大で扱う。
- 注釈を黙って読み飛ばすのは、範囲の外でエラーにする原則の例外である。注釈は印刷されないことが多く、Golem の見た目の比較の対象でもないので、例外にした。
- 描く前にすべてのページを検査するので、内容のストリームを二回解釈する。解釈は画素を描くより十分に安いと見込む。
- `DecodeParms` を拒むので、Golem が予測子つきで圧縮し始めたら、shem を直すまで止まる。それは [ADR 0001](0001-scope-golem-contract-and-determinism.md) の版の上げ方の手順どおりである。

## 参考文献

- ISO 32000-1:2008 https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf ：7.2（字句）、7.3（オブジェクト）、7.4.4（FlateDecode）、7.5（ファイルの構造）、7.7.3（ページの木と継承）、7.8（内容のストリーム）、附属書 L の図 9（演算子を置ける場所。8.2 から参照される）
- RFC 1950 ZLIB Compressed Data Format Specification version 3.3 https://www.rfc-editor.org/rfc/rfc1950
- RFC 1951 DEFLATE Compressed Data Format Specification version 1.3 https://www.rfc-editor.org/rfc/rfc1951
- Golem の事実の調査（Golem の origin/main 1254092a）：[Writer.hs:228-289](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L228-L289)、[Writer.hs:330-350](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L330-L350)、[Writer.hs:403-435](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L403-L435)、[Drawing.hs:78-127](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Drawing.hs#L78-L127)
