# ADR 0004: 文字のアウトラインの取得、CID の写像、縦組み

ステータス: 採択

## 文脈

Golem は、すべてのフォントを Type 0 のフォントと CIDFont で埋め込む（Golem の origin/main 1254092a の [Writer.hs:575-622](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L575-L622)）。
アウトラインの形は二つある。

| 輪郭 | CIDFont の `Subtype` | プログラム | `CIDToGIDMap` | 文字のコード |
|---|---|---|---|---|
| CFF | `CIDFontType0` | `FontFile3`、`/Subtype /CIDFontType0C`。OpenType から `CFF ` の表だけを取り出した裸の CFF | 書かない | CID-keyed の CFF は CID、そうでない CFF はグリフ ID |
| TrueType | `CIDFontType2` | `FontFile2` | `/Identity` | グリフ ID |

符号化は `Identity-H`、縦組みのフォントでは `Identity-V` である。
縦組みのフォントは `DW2 [880 -1000]` と、`W2` を `[g g -v 500 880]` の形で書く（[Writer.hs:602-606](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L602-L606)）。
文字のコードは 2 バイトの 16 進で、一つの `Tj` に一字だけを置き、字ごとに `Tm` で位置を決める。
斜体の合成は `Tm` の傾きで、太字の合成は `2 Tr` と `w` で表す。
縦組みの中の欧文は、`Identity-H` のフォントを `Tm` で 90° 回して置く。

CID-keyed の CFF では、CID から CFF の charset でグリフ ID を引く必要がある（ISO 32000-1 の 9.7.4.2）。
Noto Serif JP では CID とグリフ ID が一致しない（グリフ 466 の CID は 722）。
PDFium がここを誤って和文を描かなかった件が、[golem-pdf ADR 0011](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/docs/golem-pdf/adr/0011-cff-fontfile3-subtype.md) の発端である。

アウトラインを読む方法には、二つの案がある。

- **自前で書く**：CFF の Type 2 charstring の解釈器と、TrueType の `glyf` の読み取りを、shem の中に書く。
- **統合ライブラリを使う**：メンテナーが別に作るつもりでいる、HarfBuzz と FreeType を統合したライブラリを使う。まだ存在しないので、使うにはその作業を前倒しすることになる。

shem が要るのは、ヒンティングのないアウトラインだけである。
字形の整形（shaping）も、文字のコードからグリフへの `cmap` による写像も要らない。
Golem の PDF は、グリフ ID か CID を直接書くからである。

## 実装すること

### 方式の決定

- アウトラインの読み取りは、shem の中に自前で書く。統合ライブラリは待たない。
- フォントを読むモジュールは、「フォントのプログラムのバイト列とグリフ ID から、グリフの空間のパスを返す」という一つの境界だけを外に見せる。統合ライブラリができたら、この境界の裏で置き換えられるようにする。置き換えは、画素が変わるなら [ADR 0001](0001-scope-golem-contract-and-determinism.md) のとおりマイナーバージョンを上げる。
- ヒンティングはしない。アウトラインをそのまま使う。

### フォントの資源

- `Tf` が指すフォントは、`/Subtype /Type0` で、`Encoding` が名前 `/Identity-H` か `/Identity-V` で、`DescendantFonts` が一つの CIDFont の配列であるものに限る。それ以外（単純なフォント、埋め込みの CMap など）はエラーにする。
- 文字のコードは、文字列を 2 バイトずつ区切った大きいほうが先の整数とし、それを CID とする（`Identity-H` と `Identity-V` の定義）。文字列の長さが奇数ならエラーにする。
- CIDFont が `CIDFontType0` なら、`FontDescriptor` の `FontFile3` の `Subtype` が `CIDFontType0C` であるものに限る。`OpenType` などはエラーにする。
- CIDFont が `CIDFontType2` なら、`FontFile2` を持ち、`CIDToGIDMap` がないか `/Identity` であるものに限る。ストリームの `CIDToGIDMap` はエラーにする。
- フォントのプログラムが埋め込まれていなければエラーにする。
- フォントは、オブジェクトの番号ごとに一度だけ読む。

### CID からグリフ ID への写像

- `CIDFontType0`：CFF の Top DICT に `ROS` があれば CID-keyed とし、charset（形式 0、1、2）から CID をグリフ ID に引く。charset にない CID はエラーにする。`ROS` がなければ、CID をそのままグリフ ID とする（ISO 32000-1 の 9.7.4.2、[golem-pdf ADR 0006](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/docs/golem-pdf/adr/0006-cid-system-info.md)）。
- `CIDFontType2`：CID をそのままグリフ ID とする。
- グリフ ID がフォントのグリフの数以上なら、エラーにする。グリフ ID 0（`.notdef`）は、フォントにあるとおりに描く。

### CFF

- 読むもの：ヘッダー、Name INDEX、Top DICT INDEX、String INDEX、Global Subr INDEX、`CharStrings`、`charset`、`Private`（`Subrs`）。CID-keyed なら、`FDArray` の各 Font DICT の `Private` と、`FDSelect`（形式 0 と 3）でグリフごとの局所のサブルーチンを選ぶ。
- 一つの CFF に複数のフォントがあれば（Name INDEX の数が 1 でない）、エラーにする。
- `CharstringType` は 2 だけとする。
- `FontMatrix` は、Top DICT と、CID-keyed なら選ばれた Font DICT の両方を考える。v0.1 では、積が `[0.001 0 0 0.001 0 0]` になる場合だけを認め、それ以外はエラーにする。
- Type 2 charstring の演算子は、パスの演算子（`rmoveto hmoveto vmoveto rlineto hlineto vlineto rrcurveto hhcurveto hvcurveto vhcurveto vvcurveto rcurveline rlinecurve flex hflex hflex1 flex1`）、`endchar`、`callsubr callgsubr return`、ヒントの演算子（`hstem vstem hstemhm vstemhm hintmask cntrmask`）を解釈する。
  - ヒントは数えるだけで使わない。`hintmask` と `cntrmask` の後ろのバイトの数は、それまでのステムの数（`hintmask` の前の暗黙の `vstem` を含む）から決める。
  - 最初のスタックの演算子の前にある幅の値は、読み捨てる。
  - 数は、整数と 16.16 の固定小数点（`255` の接頭辞）を読む。座標は 16.16 のまま足し合わせる。
  - サブルーチンの番号には、サブルーチンの数に応じたバイアス（107、1131、32768）を足す。呼び出しの深さが 10 を超えたらエラーにする。
  - 算術と記憶の演算子（`abs add sub div neg random mul sqrt drop exch index roll dup put get and or not eq ifelse`）と、4 つの引数の `endchar`（`seac` の形）は、エラーにする。`random` は結果が決まらないので、範囲を広げても認めない。
  - 引数のスタックは 48 個まで（Type 2 の上限）とし、超えたらエラーにする。

### TrueType

- 読む表は `head`（`unitsPerEm`、`indexToLocFormat`）、`maxp`（グリフの数）、`loca`、`glyf` だけとする。
- 単純なグリフ：輪郭の端点、フラグ（繰り返しを含む）、x と y の座標（短い形と同じ値の省略を含む）を読み、2 次のベジエ曲線のパスにする。曲線の上にない点が続くときは、その中点を曲線の上の点とする。輪郭がすべて曲線の上にない点のときは、最初の二点の中点から始める。
- 2 次の曲線は、3 次の曲線に変換してから平坦化する（[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md)）。変換の制御点は `p0 + 2(q − p0)/3` と `p2 + 2(q − p2)/3` とし、Q32.32 で計算する。
- 複合のグリフ：`ARG_1_AND_2_ARE_WORDS`、`ARGS_ARE_XY_VALUES`、`WE_HAVE_A_SCALE`、`WE_HAVE_AN_X_AND_Y_SCALE`、`WE_HAVE_A_TWO_BY_TWO`（F2Dot14）、`MORE_COMPONENTS` を扱う。`ARGS_ARE_XY_VALUES` のない点の合わせはエラーにする。`ROUND_XY_TO_GRID`、`USE_MY_METRICS`、`SCALED_COMPONENT_OFFSET`、`UNSCALED_COMPONENT_OFFSET` は、v0.1 では `SCALED_COMPONENT_OFFSET` をエラーにし、ほかは無視する。入れ子の深さが 8 を超えたらエラーにする。
- グリフの空間から文字の空間への行列は `[1/unitsPerEm 0 0 1/unitsPerEm 0 0]` とする。

### 文字の描画

- 文字の状態は、`Tf`（フォントと大きさ `Tfs`）、`Tz`（横の拡大率 `Th`、百分率を 100 で割ったもの）、`Tr`（0 か 2）をグラフィックスの状態に持ち、`q` と `Q` で保存と復元をする。`Tc`、`Tw`、`TL`、`Ts` は 0 のままである（[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md) で範囲の外）。
- `BT` で文字の行列 `Tm` を単位行列にし、`Tm` の演算子で置き換える。
- 文字の描画の行列を `Trm = [Tfs·Th 0 0 Tfs 0 0] × Tm × CTM` とする（ISO 32000-1 の 9.4.4）。グリフのパスは、グリフの空間から文字の空間への行列、`Trm` の順で装置の空間に写す。
- 横組み（`Identity-H`）：グリフの原点を、文字の空間の原点に置く。
- 縦組み（`Identity-V`）：グリフを、位置ベクトル `v = (vx, vy)` の分だけ戻した位置に置く。すなわち、文字の空間で `(−vx/1000·Tfs, −vy/1000·Tfs)` だけ平行移動してから描く（ISO 32000-1 の 9.7.4.3）。
  - `v` は、`W2` にその CID があればその値とし、なければ `DW2` から `vy` を、横の幅 `w0` の半分を `vx` とする。`DW2` がなければ `[880 −1000]` とする。
  - `w0` は `W` にその CID があればその値、なければ `DW`（既定は 1000）とする。
- `Tj` の後、字送りの分だけ `Tm` を進める（ISO 32000-1 の 9.4.4）。横組みでは `tx = w0/1000·Tfs·Th`、縦組みでは `ty = w1/1000·Tfs` とする。`w1` は `W2` か `DW2` から取る。一つの `Tj` に複数の字があるときも、字ごとに進める。
- `0 Tr` はグリフを nonzero で塗る。`2 Tr` は塗った後に、同じグリフのパスを、そのときの線の幅と CTM で線として描く（[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md)）。線の幅は、文字の行列ではなく CTM で装置の空間に写す（ISO 32000-1 の 9.3.6）。
- 塗りと線の色は、それぞれ塗りの色と線の色とする（[ADR 0005](0005-device-colors-and-cmyk-conversion.md)）。
- グリフの描画を画素の単位で使い回す仕組み（グリフのキャッシュ）は作らない。字ごとに、アウトラインを写して塗る。

## 実装しないこと

- ヒンティング（CFF のヒント、TrueType の命令）。
- 単純なフォント（Type 1、TrueType、Type 3）、埋め込みの CMap、`Identity-H` と `Identity-V` 以外の CMap。取り込んだ PDF の図版の中のフォントは、図版を箱で描くので読まない。
- `cmap`、`hmtx`、`vmtx` など、アウトライン以外の表。字の幅は PDF の `W`、`W2` から取る。
- CFF2、OpenType の可変フォント、カラーのフォント。
- `Tr` の 0 と 2 以外（線だけ、見えない文字、切り抜き）。
- 統合ライブラリへの置き換え。できた後に別の ADR で決める。

## テスト設計

### CFF

- Type 2 charstring の各パスの演算子について、引数の数の変種（`hhcurveto` の先頭の dy の有無、`hvcurveto` の末尾の引数の有無など）を、手で組んだ charstring で確かめる。期待するパスは、Type 2 の仕様の記述から手で書く。
- `callsubr` と `callgsubr` のバイアスの三つの境界（サブルーチンの数が 1239 と 1240、33899 と 33900）。
- `hintmask` のバイトの数：ステムが 8 本と 9 本のとき、暗黙の `vstem` があるとき。
- 幅の値がある charstring とない charstring で、同じパスになる。
- 算術の演算子、`random`、`seac` の形の `endchar`、深すぎる呼び出し、スタックのあふれは、エラーになる。
- charset の形式 0、1、2 で、CID からグリフ ID を引ける。charset にない CID はエラーになる。
- Golem の fixture に埋め込まれた Noto Serif JP の CFF で、CID 722 がグリフ 466 に引かれる。
- `FDSelect` の形式 0 と 3 で、グリフごとに Font DICT が選ばれ、その局所のサブルーチンが呼ばれる。

### TrueType

- 単純なグリフ：曲線の上にない点の連続、すべてが曲線の上にない輪郭、フラグの繰り返し、短い座標と省略された座標。
- 複合のグリフ：平行移動だけ、一様の拡大、x と y の拡大、2 × 2 の行列、入れ子。点の合わせと `SCALED_COMPONENT_OFFSET` はエラーになる。
- `loca` の短い形と長い形、長さ 0 のグリフ（空白）。

### 文字の描画

- 横組み：`1 0 0 1 x y Tm` の一字が、グリフの原点を `(x, y)` に置く。`Tz 50` で横が半分になる。
- 斜体の合成：`1 0 0.2 1 x y Tm` で、グリフの上が右に傾く。
- 縦組み：`W2` のある CID と、`DW2` だけに頼る CID で、グリフが `v` だけ戻った位置に置かれる。`DW2` がない縦組みのフォントは `[880 −1000]` を使う。
- 縦組みの中の欧文：`0 -1 1 0 x y Tm` の一字が、時計回りに 90° 回る。
- `2 Tr` の太字の合成で、`0 Tr` より輪郭が線の幅の半分だけ太る。
- 一つの `Tj` に二字があるとき、二字目が字送りの分だけ進む（横組みと縦組みの両方）。
- 奇数の長さの文字列、範囲の外のグリフ ID、`Encoding` が CMap のストリームのフォント、`OpenType` の `FontFile3`、`3 Tr` は、エラーになる。

### 外のビューアーとの照合

- 正しさは自動のテストでは確かめられないので、外のビューアーと目で比べる項目を、[ロードマップの「目視検査の項目」](../roadmap.md#目視検査の項目)に書く。Golem の fixture（和文の縦組み、横組み、合成の太字と斜体を含むもの）を shem と pdftoppm で描いて並べ、字の抜け、字の取り違え、位置のずれがないことを、リリースの前にメンテナーが確かめる。

## トレードオフ

- 自前で書くと、CFF の charstring の解釈器の分の量が要る。そのかわり、統合ライブラリの完成を待たずに進められ、固定小数点だけで書けるので [ADR 0001](0001-scope-golem-contract-and-determinism.md) の決定性の約束を守りやすい。統合ライブラリを使う案は、FreeType の固定小数点の計算をそのまま使える利点があるが、ライブラリの版の更新で画素が変わりうる。
- 統合ライブラリができた後も、shem の自前の実装を残すか置き換えるかは、そのときに決める。境界を一つに絞るので、置き換えの手間は小さい。
- ヒンティングをしないので、小さな解像度では、PDFium や pdftoppm（FreeType のヒンティングを使う設定のとき）と字の形が違う。見た目の変化の検出には、毎回同じ形で描けば足りる。
- 算術の演算子を拒むので、それを使う CFF（古いフォントの一部）は描けない。Golem が埋め込むサブセットに現れたら、範囲を広げる ADR を書く。`random` だけは、決定性のために認めない。
- `FontMatrix` を 0.001 だけに限るので、1000 以外の em の CFF は描けない。現れたら範囲を広げる。
- 字の幅を PDF の `W` から取るので、`W` とフォントの中の幅が食い違っていても気付かない。Golem は字ごとに `Tm` を置くので、幅はページの上の位置に効かない。

## 参考文献

- ISO 32000-1:2008 https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf ：9.3（文字の状態）、9.4（文字のオブジェクト、9.4.4 の文字の描画の行列）、9.7.4.2（CIDFont のグリフの選択）、9.7.4.3（CIDFont の字の幅と縦組みの位置ベクトル）、9.7.5.2（Identity-H と Identity-V）
- Adobe Technical Note #5176, The Compact Font Format Specification https://adobe-type-tools.github.io/font-tech-notes/pdfs/5176.CFF.pdf
- Adobe Technical Note #5177, The Type 2 Charstring Format https://adobe-type-tools.github.io/font-tech-notes/pdfs/5177.Type2.pdf
- OpenType の仕様：`glyf` https://learn.microsoft.com/en-us/typography/opentype/spec/glyf 、`loca` https://learn.microsoft.com/en-us/typography/opentype/spec/loca 、`head` https://learn.microsoft.com/en-us/typography/opentype/spec/head
- [golem-pdf ADR 0006](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/docs/golem-pdf/adr/0006-cid-system-info.md)（CIDSystemInfo と文字のコード）、[golem-pdf ADR 0011](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/docs/golem-pdf/adr/0011-cff-fontfile3-subtype.md)（CFF の埋め込み）
