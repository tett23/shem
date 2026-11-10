# ADR 0027: 画像と図版の中身を描く

ステータス: 採択→決めていなかった細部は[ADR 0028](0028-details-left-open-by-images-and-figures.md)、[ADR 0029](0029-details-left-open-by-images-and-forms.md)

## 文脈

shem は、画像と図版（Form XObject）を、中身を読まずに灰色の箱とバツ印で描く（[ADR 0006](0006-placeholder-boxes-for-images-and-forms.md)）。
Golem の見た目の回帰テストは、Golem の PDF を shem で描いた画像の SHA-256 を参照と比べる。
箱で描くかぎり、画像や図版の中身が変わっても参照は変わらず、テストは変化を見逃す。

Golem は、見た目の回帰テストで画像と図版の中身を描くことを求めている。
求めは Golem の ADR 0294 にまとめられているが、ADR 0294 はまだ「提案」でコミットされていない。
この ADR は、Golem の Writer のコミット済みのコードと、Golem の側の引き継ぎの資料（2026-11-10、Golem の origin/main の `b35fd7b9` で調べたもの）から確かめた事実を書く。
ADR 0294 が採択されたら、リンクを足す。

Golem の求めは次の五つである。

1. Golem が書くすべての組み合わせの画像を復号して描く。
2. SVG の図版の Form の演算子をすべて描く（`W n` の切り抜きを含む）。
3. 見た目の回帰テストの入力にある PDF の図版（今は `figures` の `images/chart.pdf`）を描く。
4. 1 から 3 の外の入力は、今のとおり、ファイルを書かずに終了コード 1 で終える。箱で代わりに描かない。
5. x86-64 と aarch64 で同じ画素にする。画像の拡大縮小の標本の取り方も整数で決める。

### 画像

Golem の Writer は画像を [`imageObjects`](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L1209-L1251) で書き、ページには `q <w> 0 0 <h> <x> <y> cm /ImN Do Q` で置く（[Writer.hs:480-481](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L480-L481)）。

- **フィルタ：** JPEG は受け取ったバイト列のまま `DCTDecode`、PNG は標本を取り出して `FlateDecode` で書く。どちらも `DecodeParms` を持たない。
- **`BitsPerComponent`：** PNG の深さ（1、2、4、8、16）のまま、JPEG は SOF の精度のままである。
- **色空間：** `DeviceGray`、`DeviceCMYK`、`[/ICCBased n]`（成分の数 1、3、4）、`[/Indexed [/ICCBased n] <hival> <16進の表>]` を使う。プロファイルを持たない RGB とパレットには、文書に一つの sRGB のプロファイル（`/N 3`）を当てる。
- **`Decode`：** Adobe の反転した CMYK の JPEG には `/Decode [1 0 1 0 1 0 1 0]` を付ける。
- **透過：** `SMask`（`DeviceGray`、`FlateDecode`、`BitsPerComponent 8`）で書く。16 ビットの画像でも `SMask` は 8 ビットである。

画像は拡大して置かれ、縦組みの見本では 90 度回して置かれる（2 × 2 画素を 300 × 150 ポイントに置く例がある）。
Golem の JPEG の読み込みは、SOF のうち `C4`、`C8`、`CC` を除くすべてを受け付けるように読める（[Image.hs:76-108](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Image.hs#L76-L108)）。
fixture の JPEG（`figures/images/photo.jpg`）は、ベースライン、8 ビット、2 × 3 画素、成分 3、標本化 4:2:0 で、MCU に満たない端の画素と色差の補間を含む。

### SVG の図版

SVG の図版は、Golem が内容を書く Form XObject である（[Drawing.hs:77-127](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Drawing.hs#L77-L127)、辞書は [`drawnObject`](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L1066-L1077)）。

- 辞書は `/BBox [0 0 <w> <h>]`、`/Matrix [1/w 0 0 1/h 0 0]`（10 桁に丸めた小数）、`/Resources << /ColorSpace << /GolemSRGB [/ICCBased n] >> >>` である。
- 内容の先頭は `1 0 0 -1 0 <h> cm` と `/GolemSRGB cs /GolemSRGB CS` である。
- パスごとに `q`、塗りの `r g b sc`、線の `r g b SC <w> w <0|1|2> J <0|1|2> j <M> M [<配列>] <位相> d`、パス（`m l c h`）、`f`、`f*`、`S`、`B`、`B*`、`n` のどれか、`Q` を書く。
- グループは `q <a b c d e f> cm` で、切り抜きがあれば `<x> <y> <w> <h> re W n` を書く。SVG の `transform` の行列がそのまま入る。
- 文字、透明、パターン、シェーディングは書かない。

### PDF の図版

取り込んだ PDF のページは、元の内容をほぼそのまま持つ Form になる（[`formObject`](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L1082-L1101)）。
内容は Flate で圧縮し直し、`DeviceRGB` を sRGB の `ICCBased` に替え、資源を写し、元に `/Group` があれば写す。
どの演算子と資源が現れるかは、元の PDF を書いた道具で決まる。
`figures` の `images/chart.pdf` の Form の内容は `k`、`re f`、`CS SC`、`w`、`m l S` だけである。

### 今の shem に足りないもの

Golem の側で、手元で組んだ五つの入力の Form に現れた演算子は `B CS J M Q S SC W c cm cs d f h j k l m n q re sc w` だった。
shem が読まないのは、`CS`、`sc`、`SC`、`B`、`W`、`n`、`j`、`M` と、`cs` と `CS` の `ICCBased` である。
Writer は、ほかに `f*`、`B*`、`2 J`、`1 j`、`2 j` も書きうる（Drawing.hs のコードから読んだ。fixture には現れなかった）。

線には、もう一つの問題がある。
shem は、CTM が角を保つ（`a == d && −b == c` か `−a == d && b == c` が厳密に成り立つ）ときだけ線を描く（[src/render.rs:139-146](https://github.com/tett23/shem/blob/33c6bec0c82fdf9e659c40938132d64ea0e338b8/src/render.rs#L139-L146)）。
SVG の図版の Form では、置く行列と `Matrix` の積の対角が `300 × 0.0041666667` と `150 × 0.0083333333` で、厳密には等しくない。
SVG の `transform` に、縦横で違う拡大や斜めの変形があれば、丸めと関係なく角を保たない。
そのため、図版の線は、今の検査ではほぼ必ずエラーになる。

### tett23 が決めたこと（2026-11-10）

- 角を保たない CTM での線は、一般のアフィン変換として描く。
- 画像を装置の画素に写す標本の取り方は、双線形の補間とする。
- `ICCBased` は、プロファイルを使わず、成分の数で扱う。
- JPEG は、ベースライン、プログレッシブ、12 ビットの拡張まで読む。
- 切り抜きの縁は、塗りと同じアンチエイリアスの覆いにする。
- 画像と図版は、一つの ADR で決める。

## 実装すること

### 範囲

- 画像の XObject と Form XObject の中身を描く。[ADR 0006](0006-placeholder-boxes-for-images-and-forms.md) の箱は描かない。
- この ADR の範囲の外の画像と Form は、[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md) のとおり、画素を描かない検査で見つけ、出力を一つも書かないうちにエラーで止める。箱で代わりに描くことはしない。

### 演算子

[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md) の表に、次の演算子を加える。
ページの内容と Form の内容は、同じ規則で読む。

| 分類 | 演算子 | 被演算子 |
|---|---|---|
| グラフィックスの状態 | `j` | 整数 1（0 miter、1 round、2 bevel） |
| | `M` | 数 1（1 以上） |
| | `J` | 整数 1（2 square を足す） |
| 色 | `CS` | 名前 1 |
| | `sc`、`SC` | 色空間の成分の数だけの数 |
| パスの塗り | `f*`、`B`、`B*`、`n` | なし |
| 切り抜き | `W`、`W*` | なし |

- `j` の 0 から 2 の外の整数は `malformed j <値>`、1 未満の `M` は `malformed M` のエラーにする。初期値は `0 j` と `10 M` である。
- `CS` は線の色空間を選ぶ。`cs` と `CS` が選べるのは、`Resources` の `ColorSpace` にある `ICCBased`（`N` が 1、3、4）と、今の Separation である。`sc` と `SC` は、色空間が `ICCBased` か Device の色空間のときだけ認め、成分の数が違えばエラーにする。`scn` は今のとおり Separation に限る。
- `B` と `B*` は、塗ってから線を描く。塗りの規則は、`B` が nonzero、`B*` が偶奇とする。`f*` は偶奇の規則で塗る。`n` は何も描かずにパスを空にする。
- `W` と `W*` は、パスを終える演算子（`f`、`f*`、`S`、`B`、`B*`、`n`）の直前にだけ置ける。その演算子でパスを描いた後に、パスで切り抜きを狭める。
- `s`、`b`、`b*`、`g`、`G`、`rg`、`RG`、`gs`、`sh`、インライン画像は、今のとおりエラーにする。

### 色

- `ICCBased` の色空間は、プロファイルを読まず、`N` が 1 なら DeviceGray、3 なら RGB、4 なら DeviceCMYK と同じ変換で 8 ビットの RGB にする。`Alternate` と `Range` は読まない。`N` がほかの値ならエラーにする。
- RGB は、各成分を [ADR 0005](0005-device-colors-and-cmyk-conversion.md) と同じ `round(x × 255)`（同じ近さなら偶数）で 0 から 255 にする。

### 線

- 線の角は、`0 j` を今の miter、`2 j` を今の bevel とし、`1 j` は頂点を中心とする半径 `half` の円板を加える（[ADR 0026](0026-corrections-to-round-caps-and-dashes.md) の円板）。
- miter の上限は `M` とする。二つの区間の方向の内積を `dot`、長さの積を `lens` として、`M² · (lens + dot) ≥ 2 · lens` なら miter、そうでなければ bevel とする。`M` が 10 なら、今の `100 · dot + 98 · lens ≥ 0` と同じ条件である。整数で、`M` の 2 乗を 2^64 倍した値で比べる。
- `2 J`（square）は、開いた部分パスと破線の両端を、区間の向きに `half` だけ延ばした butt の線端とする。延ばす量は、法線と同じく丸めた方向で求める。

### 一般のアフィン変換での線

- CTM が角を保つ線は、今の経路で描く。`J`、`j`、`M` を使わないページの画像は変わらない。
- 角を保たない CTM での線は、利用者の空間で線の多角形を作り、その点を CTM で装置の空間に写して丸める。
  - 利用者の空間の点は、今の `Fixed`（2^−32 単位）で持つ。
  - 曲線は利用者の空間で平坦化する。止める条件は、制御点の二階差分に CTM の `sqrt(a² + b² + c² + d²)` の上界を掛けた値が 1/16 画素以下になることとする。
  - 線の幅の半分 `half` と破線の長さは、利用者の空間の値のまま使う。線端の円板と継ぎ目は利用者の空間の円で作るので、装置の空間では楕円になる。
  - 線の幅が 0 の線は `zero-width stroke with a CTM that does not keep angles` のエラーにする。
  - 多角形の点を装置の空間に写した値が `2^31 − 1` を超えれば、今と同じ `stroke outside the range of coordinates` のエラーにする。
- 一般の経路では、キャンバスの外の平坦化を省かない（[ADR 0022](0022-no-work-for-geometry-off-the-canvas.md) の省き方は、角を保つ経路だけに当てる）。

### 切り抜き

- 切り抜きは、グラフィックスの状態に入れ、`q` と `Q` で保存と復元をする。初期値は、切り抜きなしである。
- `W` と `W*` は、パスを、塗りと同じ方法（[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md) の覆い）で 0 から 255 の覆いにする。新しい切り抜きの各画素は、今の切り抜きとの積 `div_round(c × p, 255)` とする。
- 塗り、線、画像の合成では、覆い（画像では不透明度との積）にさらに切り抜きを掛ける。掛け算はどれも `div_round(x × y, 255)` とする。
- 切り抜きの覆いは、それが 0 でない画素の外接矩形の中だけに持つ。外接矩形の外は 0 とする。作業用の領域は、[ADR 0013](0013-fewer-allocations-for-musl-threads.md) のとおり、スレッドの `Workspace` で使い回す。

### Form XObject

- `Do` の Form は、`q`、`Matrix`（省略時は単位行列）を CTM に掛ける、`BBox` の矩形で切り抜く、内容を解釈する、`Q`、の順に描く（ISO 32000-1 の 8.10.1）。グラフィックスの状態は、`Do` のときのものを受け継ぐ。
- 内容の名前は、Form の `Resources` から引く。`Resources` のない Form はエラーにする。
- Form の中では、`Do` で画像と Form を置ける。Form の入れ子は、ページを 0 段目として 8 段までとし、超えれば `forms nested deeper than 8` のエラーにする。
- Form の中の文字のオブジェクト（`BT`）は、`text in a form` のエラーにする。
- `Group`、`Ref`、`OC` を持つ Form と、`FormType` が 1 でない Form は、範囲の外としてエラーにする。

### 画像の XObject

- 読む辞書の項目は、`Width`、`Height`、`BitsPerComponent`、`ColorSpace`、`Filter`、`Decode`、`SMask` である。`ImageMask`、`Mask`、`Interpolate` が真、`Intent` 以外の未知の項目のある画像は、エラーにする。
- フィルタは、なし、`FlateDecode`、`DCTDecode` のどれか一つとする。`DecodeParms` はエラーにする。
- `Width × Height × 成分の数` は 2^26 以下とする。超えれば `image larger than 67108864 samples` のエラーにする。
- 色空間は、`DeviceGray`、`DeviceCMYK`、`ICCBased`（`N` が 1、3、4）と、それらを基にする `Indexed` とする。`Indexed` の表は文字列で、長さが `(hival + 1) × 成分の数` でなければエラーにする。範囲の外の番号の標本はエラーにする。
- `BitsPerComponent` は、Flate の標本では 1、2、4、8、16（`Indexed` は 1、2、4、8）、JPEG では SOF の精度と同じ値とする。行の端は、バイトの境目にそろえる（ISO 32000-1 の 8.9.3）。
- 標本は、色空間の成分の値にしてから 8 ビットの RGB にする。
  - `b` ビットの標本 `v` は `div_round(v × 255, 2^b − 1)` で 0 から 255 にする。16 ビットは `div_round(v × 255, 65535)` とする。
  - `Decode` は、既定の配列か、すべての組を反転した配列（`[1 0 1 0 …]`）だけを認め、反転なら `255 − v` とする。ほかの配列はエラーにする。
  - その後、[色](#色) の変換で RGB にする。CMYK の各成分は、0 から 255 のまま、[ADR 0005](0005-device-colors-and-cmyk-conversion.md) の `R = 255 − min(255, C + K)` に入れる。
- `SMask` は、`DeviceGray`、`BitsPerComponent` 8、フィルタなしか `FlateDecode` の画像だけを認める。`Matte` はエラーにする。`SMask` の大きさは、画像と違ってよい。

### JPEG

- ベースライン（SOF0）、拡張の順次（SOF1、ハフマン符号、8 ビットと 12 ビット）、プログレッシブ（SOF2、8 ビットと 12 ビット）を読む。算術符号（SOF9 から SOF15）、ロスレス（SOF3、SOF7、SOF11、SOF15）、階層（SOF5 から SOF7、SOF13 から SOF15）は、`unsupported JPEG <SOF の番号>` のエラーにする。
- 成分の数は 1、3、4 とし、標本化の係数は 1 から 4 とする。リスタートの間隔（DRI と RSTn）を読む。
- 逆 DCT は、IJG の libjpeg の `jidctint.c` の整数の式（`CONST_BITS` 13、`PASS1_BITS` は 8 ビットで 2、12 ビットで 1）と同じ計算で、標本を 0 から `2^精度 − 1` に切り詰める。式は再実装し、libjpeg を依存に入れない。
- 色差の補間は、標本化が 2×1 と 2×2 の成分では libjpeg の `jdsample.c` の fancy upsampling（三角形の重みの線形補間）と同じ式とし、ほかの比は標本の繰り返しとする。
- 色の変換は、APP14（Adobe）の `transform` が 1 なら YCbCr、2 なら YCCK、0 なら変換なしとする。APP14 がなければ、成分 3 は YCbCr、1 と 4 は変換なしとする。YCbCr から RGB への変換は、libjpeg の `jdcolor.c` の整数の表（`SCALEBITS` 16）と同じ式とする。
- 12 ビットの標本は、色の変換の後に `div_round(v × 255, 4095)` で 8 ビットにする。
- 画像の辞書の `Width`、`Height`、成分の数、`BitsPerComponent` が SOF と合わなければエラーにする。

### 画像を描く

- 画像は、画像の空間の単位正方形を CTM で写した平行四辺形を、[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md) の塗りと同じ覆いで描く。縁のアンチエイリアスは、この覆いで決まる。
- 平行四辺形の中の各画素の色は、双線形の補間で求める。
  - 画素の中心 `(x + 1/2, y + 1/2)` を、CTM の逆で画像の空間に写し、標本の単位の座標 `(u, v)` にする。標本 `(i, j)`（`j` は上の行から数える）の中心は `(i + 1/2, j + 1/2)` とする。逆の写像は、CTM の成分から有理数として求め、2^−16 標本の単位に切り捨てる。
  - `u − 1/2` と `v − 1/2` の整数部を `i0`、`j0`、端数を `fx`、`fy`（2^−16 単位）とし、四つの標本 `(i0, j0)`、`(i0 + 1, j0)`、`(i0, j0 + 1)`、`(i0 + 1, j0 + 1)` を、画像の縁の外は縁の標本に切り詰めて引く。
  - RGB の各成分と不透明度を、`(s00 · (2^16 − fx) + s10 · fx) · (2^16 − fy) + (s01 · (2^16 − fx) + s11 · fx) · fy` を 2^32 で割って丸めた値とする（同じ近さなら偶数）。
  - 補間は、8 ビットの RGB にした後の標本で行う。`SMask` の不透明度は、`SMask` の大きさで同じ規則で補間する。
- 画素の不透明度は、覆いと `SMask` の不透明度と切り抜きの積とし、今の合成の式 `(c × a + p × (255 − a) + 127) / 255` で重ねる。
- 画像は、塗りと同じく、平行四辺形のうちキャンバスの中の画素だけを求める。

### 検査とキャッシュ

- 画像の復号の誤り（壊れた Flate の標本、壊れた JPEG、大きさの不一致、範囲の外の番号）は、描く前の検査で見つける。検査は、画像を最後まで復号して確かめる。
- 復号した画像は、`Session` の中で、オブジェクトの番号ごとに一度だけ作って共有してよい。共有するかどうかで、出力は変わらない。

### fixture と文書

- `tests/fixtures/golem/figures.pdf` を、今の Golem で作りなおす。PNG の tRNS の画像が増えている。`complex-horizontal`、`complex-vertical` の画像と図版の PDF も足す。Golem の版は、fixture の README に記録する。
- 仕様の「画像と図版」の節を書きなおし、演算子の表、色、線の項を直す。ロードマップの v0.x の画像と図版の項を、実装したものに直す。
- [ADR 0006](0006-placeholder-boxes-for-images-and-forms.md) のステータスを「採択→画像と図版の描き方はADR 0027」にする。
- Golem の ADR 0294 が採択されたら、この ADR にリンクを足す。

## 実装しないこと

- ICC のプロファイルによる色の変換。
- `/Interpolate` の真と、ほかの標本の取り方（最近傍、縮小のときの面積の平均）。縮小でも同じ双線形の補間を使う。
- `ImageMask`（ステンシルの画像）、`Mask`（色の範囲と画像のマスク）、`SMask` の `Matte`、ExtGState の `SMask`（`gs`）。
- 透明のグループ（Form の `Group`）、パターン、シェーディング、インライン画像。
- Form の中の文字。
- JPEG の算術符号、ロスレス、階層の符号化と、`JPXDecode`、`JBIG2Decode`、`CCITTFaxDecode`。
- 一般のアフィン変換での線の、キャンバスの外の平坦化の省略。
- 画像の拡大と縮小の速さのための工夫（たとえば、同じ行の標本の再利用）は、出力を変えない範囲で、実装のときに選ぶ。

## テスト設計

どのテストも、実装の前に書き、今のコードで失敗することを確かめてから実装する。

### 演算子と色

- `j`、`M`、`2 J`、`CS`、`sc`、`SC`、`f*`、`B`、`B*`、`n`、`W n`、`W* n` が、正しい被演算子で通り、誤った被演算子でエラーになる。`3 j`、`0.5 M` はエラーになる。
- `W` がパスを終える演算子の直前にないとエラーになる。
- `ICCBased` の `N` が 1、3、4 の色が、それぞれ DeviceGray、RGB、DeviceCMYK と同じ画素になる。`N` が 2 はエラーになる。

### 線

- `M` を 10 にした線の画像は、今の画像とバイト単位で同じである。`J`、`j`、`M` を使わない既存の fixture の画像は、72、97、150、300 dpi の PPM と 150、300 dpi の PNG で main と同じバイト列である（箱を描く画像と図版のページを除く）。
- `1 j` の直角の折れ線は、外側の角が半径 `half` の四分円になる。`2 J` の線は、端点から `half` だけ延びる。
- `M` の境目：直角（miter の比は √2）の継ぎ目は、`1.41 M` で bevel、`1.42 M` で miter になる。
- 一般のアフィン変換：`2 0 0 1 0 0 cm` の下の円の線は、横の太さが縦の 2 倍になる。角を保つ CTM で描いた線を、`1.0000001` 倍だけずらした CTM で描いた線と比べ、違う画素が 1 画素の幅の縁だけにある。斜めの変形（`1 0 0.5 1 0 0 cm`）の線が描ける。
- 一般のアフィン変換での幅 0 の線と、装置の空間で範囲を超える線はエラーになる。

### 切り抜き

- `0 0 10 10 re W n` の後の大きな矩形の塗りは、10 × 10 の範囲だけを塗る。縁の画素の覆いは、同じ矩形を塗ったときと同じである。
- 切り抜きは `q` と `Q` で戻り、二つの `W` は積になる。
- 斜めの切り抜きの縁の画素の値が、切り抜きの覆いと塗りの覆いの積 `div_round(c × p, 255)` になる。

### 画像

- 1、2、4、8、16 ビットの `DeviceGray`、`ICCBased` の 1、3、4 成分、`Indexed`、`Decode` の反転の、2 × 2 画素の画像を、拡大せずに置いた画素が、標本の変換の式と一致する。
- 2 × 2 画素の画像を 300 × 150 ポイントに置くと、画素の色が双線形の補間の式と一致する。縁の外の標本は切り詰める。画像を 90 度回して置いた画像は、回さずに置いて描いた画像を回したものと同じである。
- `SMask` の画像は、不透明度の補間と合成の式と一致する。`SMask` の大きさが画像と違っても描ける。
- 範囲の外の番号の `Indexed`、`Decode` の一般の配列、`ImageMask`、`Matte`、大きすぎる画像は、エラーになり、出力のファイルを作らない。

### JPEG

- テストの中に、逆 DCT と色の変換を有理数で求める遅い参照を置き、8 × 8 のブロックの乱数の係数で、整数の逆 DCT が参照と各標本 1 以内で一致することを確かめる。
- libjpeg の `jidctint.c` の式で作った既知の入力と出力の組（テストの中に数値で書く）と、ちょうど一致する。
- ベースライン、プログレッシブ、12 ビットの拡張の順次、12 ビットのプログレッシブ、リスタートの間隔のある JPEG、成分 1 と 4、APP14 の transform 0、1、2、標本化 4:4:4、4:2:2、4:2:0、端の MCU を持つ大きさの JPEG を、テストの中で作るか fixture に置き、pdftoppm と djpeg（どちらも libjpeg の系統）で復号した標本と比べる。比べる相手と一致の程度（ちょうど一致か、何段階以内か）を、プルリクエストに書く。
- 算術符号とロスレスの JPEG は `unsupported JPEG` のエラーになる。壊れた JPEG（途中で切れたもの、ハフマンの表のない符号）はエラーになり、出力のファイルを作らない。

### Form

- `BBox` の外は描かれない。`Matrix` が CTM に掛かる。グラフィックスの状態は `Do` の後に戻る。
- 9 段の入れ子の Form は `forms nested deeper than 8` のエラーになり、8 段は描ける。自身を `Do` する Form も、このエラーで止まる。
- Form の中の `BT` は `text in a form`、`Group` を持つ Form はエラーになる。

### Golem の出力

- 作りなおした `figures.pdf` と、`complex-horizontal`、`complex-vertical` の PDF が、エラーなしに描ける。
- 72、150、300 dpi のゴールデンを作る前に、pdftoppm で描いた画像と並べて目で比べ、画像の位置と向き、図版の線と塗りと切り抜き、透過が合うことを確かめ、プルリクエストに書く。拡大した画像の補間は pdftoppm と違いうるので、違いがあれば書く。ロードマップの目視検査の「画像と図版の箱」の項を「画像と図版」に直す。
- 結合テスト `tests/allocations.rs` に、画像の数と画素の数に割り当ての回数が比例しないことを足す（[ADR 0013](0013-fewer-allocations-for-musl-threads.md)）。
- CI のゴールデンで、x86-64 と aarch64、Linux と macOS で同じバイト列になることを確かめる。

## トレードオフ

- 箱をやめるので、画像と図版を持つページの画像は、すべて変わる。Golem の参照も作りなおしになる。
- 双線形の補間は、最近傍より規則が多い（縁の切り詰め、端数の丸め）。そのかわり、小さな画像を大きく置いたときに、pdftoppm などの描画系の見た目に近い。縮小のときに面積の平均を取らないので、大きな画像を小さく置くと、細かい模様が崩れうる。
- `ICCBased` のプロファイルを使わないので、色の管理をする描画系とは、色が少し違う。Golem は文書に一つの sRGB のプロファイルを当てるだけなので、違いは sRGB と、shem の RGB の扱い（変換なし）の違いに限られる。
- JPEG の逆 DCT と色差の補間を libjpeg の式に合わせるので、libjpeg の系統の描画系とは、標本がほぼ一致すると見込む。式を写すので、IJG のライセンスの表示が要るかを、実装のときに確かめる。
- 12 ビットの JPEG を読むので、逆 DCT の中間値の幅が広くなる。i32 で足りるかを、実装のときに確かめる（足りなければ i64 にする）。
- 角を保たない CTM での線は、角を保つ CTM での線と別の経路になる。二つの経路の境目（角をほぼ保つ CTM）で、画素がわずかに変わる。今の経路の画像を変えないための選択である。
- 検査で画像を最後まで復号するので、画像の多い文書では、検査の時間が延びる。キャッシュで、描画のときの復号を省ける。
- Form の中の文字と透明のグループをエラーにするので、Golem の外の道具が書いた PDF の図版は、多くが描けない。Golem の ADR 0294 は、そうした図版を見た目の回帰テストに入れないとしている。

## 参考文献

- ISO 32000-1:2008 https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf ：8.4.3（線の幅、線端、角、miter の上限）、8.5.4（切り抜き）、8.6.5.5（ICCBased）、8.6.6.3（Indexed）、8.9.5（画像の辞書）、8.10（Form XObject）、11.6.5.3（SMask）
- ITU-T T.81（JPEG）https://www.w3.org/Graphics/JPEG/itu-t81.pdf
- Independent JPEG Group の libjpeg（`jidctint.c`、`jdsample.c`、`jdcolor.c`）https://www.ijg.org/
- Adobe, Supporting the DCT Filters in PostScript Level 2, Technical Note #5116：APP14 の `transform`
- Golem の事実の調査（Golem の origin/main `b35fd7b9`）：[Writer.hs の imageObjects](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L1209-L1251)、[Writer.hs:480-481](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L480-L481)、[Drawing.hs:77-127](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Drawing.hs#L77-L127)、[drawnObject](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L1066-L1077)、[formObject](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L1082-L1101)、[Image.hs:76-108](https://github.com/tett23/golem/blob/b35fd7b94902bff96f80b077e7e3a11d16cc899d/modules/golem-pdf/src/Golem/Pdf/Image.hs#L76-L108)
- shem の今の線の検査（33c6bec0）：[src/render.rs:139-146](https://github.com/tett23/shem/blob/33c6bec0c82fdf9e659c40938132d64ea0e338b8/src/render.rs#L139-L146)
