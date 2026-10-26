# ADR 0005: 色の空間と、CMYK から RGB への変換

ステータス: 採択（未実装）

## 文脈

Golem のページの内容は、次の色を使う（Golem の origin/main 1254092a で調べた）。

- `k` と `K` の DeviceCMYK。`SetColor` は塗りと線を同じ値にする（[Writer.hs:257-259](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L257-L259)）。
- 最初の色の指定より前は、既定の DeviceGray の黒である。
- トンボは `/Reg cs 1 scn` で塗る。`/Reg` は Separation の `All` で、代替の色空間は DeviceCMYK、変換の関数は `FunctionType 2`、`C0` が 0 四つ、`C1` が 1 四つである（[Writer.hs:560-572](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L560-L572)）。

SVG の図版の Form は sRGB の ICCBased を、画像は DeviceGray、DeviceCMYK、ICCBased、Indexed を使う。
v0.1 では図版と画像を箱で描く（[ADR 0006](0006-placeholder-boxes-for-images-and-forms.md)）ので、これらの色は読まない。
Golem の Writer は `ExtGState` も `gs` も書かず、ページの内容に透明はない。

PDF/X-4 の出力は、`OutputIntents` に Japan Color 2001 Coated のプロファイルを埋め込む。
ICC のプロファイルで CMYK を RGB にすると、印刷の色に近づくが、プロファイルの解釈と浮動小数点数の計算が要る。
見た目の変化を見つける目的では、同じ CMYK が毎回同じ RGB になれば足りる。

## 実装すること

### 出力の色

- 画素は 8 ビットの RGB とする。紙は白（255、255、255）とする（[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md)）。
- 色の成分の値は Q32.32 で受け取り、0 から 1 に切り詰めてから、`round(x × 255)`（最近接の偶数への丸め）で 0 から 255 の整数にする。

### 色空間

- **DeviceGray**：灰色 `G` を `(G, G, G)` にする（ISO 32000-1 の 10.3.2）。
- **DeviceCMYK**：`(C, M, Y, K)` を、`R = 255 − min(255, C + K)`、`G = 255 − min(255, M + K)`、`B = 255 − min(255, Y + K)` にする（ISO 32000-1 の 10.3.5）。成分は上の規則で 0 から 255 の整数にしてから足す。
- **Separation**：`[/Separation 名前 代替 関数]` で、代替が DeviceCMYK か DeviceGray、関数が `FunctionType 2` で `N` が 1 のものに限る。濃度 `t` を、関数で代替の色空間の値 `C0 + t × (C1 − C0)`（成分ごと、Q32.32）にしてから、上の規則で RGB にする。`Domain` があれば `t` を切り詰める。`C0` と `C1` の既定値（`[0]` と `[1]`）も扱う（ISO 32000-1 の 7.10.3）。
- 色材の名前（`All`、`None` を含む）は、色の計算に使わない。代替の色空間で描く。

### 演算子

- `k` は塗りの色、`K` は線の色を DeviceCMYK にする。
- `cs` は塗りの色空間を、資源の `ColorSpace` にある名前の色空間にし、塗りの色をその色空間の初期値にする（Separation では 1）。`cs` の名前が `DeviceGray`、`DeviceRGB`、`DeviceCMYK`、`Pattern` のときは、v0.1 ではエラーにする。
- `scn` は、塗りの色空間が Separation のときだけ認め、被演算子は数 1 とする。
- 塗りと線の色と色空間の初期値は、DeviceGray の黒（0）とする。`q` と `Q` で保存と復元をする。
- `ExtGState`、透明、オーバープリント、ブレンドのモードは扱わない（[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md) で `gs` は範囲の外）。

## 実装しないこと

- ICC のプロファイルによる変換。`OutputIntents` は読まない。
- DeviceRGB、ICCBased、CalGray、CalRGB、Lab、Indexed、DeviceN、Pattern の色空間。図版と画像を描くときに、別の ADR で加える。そのときは、ICCBased を `N` に応じて DeviceGray、DeviceRGB、DeviceCMYK として扱う案を検討する。
- `FunctionType 2` 以外の関数と、`N` が 1 でない `FunctionType 2`。
- 色の補正（ガンマ、ICC の描画の意図）。

## テスト設計

### 変換の式

- DeviceGray：0 が `(0, 0, 0)`、1 が `(255, 255, 255)`、0.5 が `(128, 128, 128)`（127.5 の偶数への丸め）になる。範囲の外の値（−1 と 2）は切り詰められる。
- DeviceCMYK：`0 0 0 0` が白、`0 0 0 1` が黒、`1 0 0 0` が `(0, 255, 255)`、`1 1 1 1` が黒、`0.5 0 0 0.5` が `(0, 127, 127)` になる。
- Separation の `/Reg`：`1 scn` が DeviceCMYK の `1 1 1 1` と同じ黒、`0 scn` が白になる。

### 演算子

- 色を指定する前の `re f` は黒で塗られる。
- `q 1 0 0 0 k Q` の後の塗りは、`q` の前の色になる。
- `k` は線の色を変えず、`K` は塗りの色を変えない（`2 Tr` の文字で確かめる）。
- `/DeviceRGB cs`、資源にない名前の `cs`、Separation でない色空間の後の `scn`、二つの被演算子の `scn` は、エラーになる。
- `FunctionType 0` の関数を持つ Separation と、代替が DeviceRGB の Separation は、エラーになる。

## トレードオフ

- ISO 32000-1 の 10.3.5 の式は、印刷の色とはかけ離れる（`1 0 0 0 k` は純粋なシアン `(0, 255, 255)` になる）。画像を人が見て色を判断する用途には向かない。見た目の変化の検出には、同じ入力が同じ色になれば足りる。
- 10.3.5 の式は `C + K` で切り詰めるので、K が 1 のとき、ほかの成分の違いが画素に現れない。`0 0 0 1 k` と `1 1 1 1 k` の違い（墨ベタとリッチブラック）は、この変換では見つからない。この種の違いは、Golem の側で内容のストリームのゴールデンで見つける。
- 成分を先に 8 ビットにしてから足すので、10.3.5 の式を実数で計算したものと、最大で 1 だけ違う。どの機械でも同じ値になる。

## 参考文献

- ISO 32000-1:2008 https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf ：7.10.3（Type 2 の関数）、8.6.4（デバイスの色空間）、8.6.6.4（Separation）、10.3.2（DeviceGray と DeviceRGB の変換）、10.3.5（DeviceCMYK から DeviceRGB への変換）
- Golem の事実の調査：[Writer.hs:257-259](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L257-L259)、[Writer.hs:560-572](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L560-L572)、[Writer.hs:648-668](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L648-L668)
