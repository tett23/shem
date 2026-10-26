# ADR 0006: 画像と図版を灰色の箱とバツ印で描く

ステータス: 採択

## 文脈

Golem のページの内容が `Do` で呼ぶのは、画像と図版の Form XObject だけである（Golem の origin/main 1254092a で調べた）。

- 画像：`q w 0 0 h x y cm /ImN Do Q`。JPEG はバイト列をそのまま `DCTDecode` にし、PNG は標本を Flate で圧縮し直し、アルファを `SMask` にする。
- 図版：`q w 0 0 h x y cm /FmN Do Q`。Form の `Matrix` が、`BBox` を単位正方形に写す（[Writer.hs:505-540](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L505-L540)）。SVG の図版の Form と、取り込んだ PDF の図版の Form がある。

画像の中身は、Golem の出力のゴールデンが、埋め込んだストリームのハッシュで固めている。
取り込んだ PDF の図版の内容のストリームも、展開した本文がゴールデンにそのまま入る。
ただし、再圧縮した PNG の標本は、Golem の出力のゴールデンに入っていない（`figures` の fixture に PNG がない）。
Golem の側で fixture を足して直す（2026-10-26 に着手）。

SVG の図版の Form と PDF の図版の Form は、出力の PDF だけでは区別できない。
どちらも `Resources` が `<< /ColorSpace << /GolemSRGB [/ICCBased n 0 R] >> >>` だけで、`Matrix` も同じ形だからである（Golem の [pdf-figures.txt:395](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-driver/test/golden/pdf-figures.txt#L395)、[pdf-figures.txt:455](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-driver/test/golden/pdf-figures.txt#L455)）。
メンテナーは、今は区別しなくてよいと決めた。
図版を描く処理は、Golem の v1.x か v2 までにあればよいからである。

画像を復号せず、図版も描かなければ、JPEG と PNG の復号、透明、ICCBased の色、取り込んだ PDF の任意の演算子を、v0.1 の範囲から外せる。
それでも、画像と図版の置かれる位置と大きさは画像に現れるので、レイアウトの変化は見つかる。

## 実装すること

### 対象

- `Do` の名前を、ページの `Resources` の `XObject` から引く（[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md)）。
- XObject の `Subtype` が `Image` なら画像の箱、`Form` なら図版の箱を描く。それ以外（`PS` など）はエラーにする。
- 画像のストリームは伸長しない。フィルタ、色空間、`SMask`、`Decode` は読まず、検査もしない。
- Form のストリームは伸長も解釈もしない。Form の `Resources`、`Group`、入れ子の XObject は読まない。

### 箱の範囲

- 画像の箱は、画像の空間の単位正方形 `(0,0)`、`(1,0)`、`(1,1)`、`(0,1)` を、そのときの CTM で装置の空間に写した平行四辺形とする（ISO 32000-1 の 8.9.4）。
- 図版の箱は、Form の `BBox` の四隅を、`Matrix`（ないときは単位行列）、CTM の順で写した平行四辺形とする（ISO 32000-1 の 8.10.1）。`BBox` がなければエラーにする。
- 箱は、ページの範囲（`MediaBox`）で切り取る。ページの内容には切り抜きがない（[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md)）ので、ほかの切り取りはない。
- 面積が 0 の平行四辺形（行列が退化しているとき）は、何も描かない。エラーにしない。

### 箱の描き方

- 平行四辺形を、RGB の `(192, 192, 192)` で、nonzero で塗る（[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md)）。下にあった描画は見えなくなる。
- 二本の対角線（向かい合う頂点を結ぶ線分）を、RGB の `(96, 96, 96)` で、線として描く。
  - 線の幅は装置の空間で `dpi / 72` 画素（1 ポイント）とし、CTM によらない。
  - 線端は butt とする。
- 塗りと線の色は、ページのグラフィックスの状態の色によらず、上の値に固定する。
- 箱を描いても、グラフィックスの状態（色、線の幅など）は変えない。

### 区別しないこと

- SVG の図版と PDF の図版を区別しない。どちらも同じ箱で描く。
- 画像と図版の箱は、同じ色と同じ形とする。

## 実装しないこと

- 画像の復号（JPEG、Flate の標本、`SMask`、`Decode`、Indexed）と、画素の描画。Golem の v1.x か v2 に間に合うように、shem の v1 までに別の ADR で自前で作る。Golem の v2 では図版をフレームに合わせて拡大縮小するので、画像を描けないと見た目の回帰テストの役に立たなくなる。
- Form の中身の解釈。図版を描くときに、[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md) の範囲を広げる ADR を書く。汎用のラスタライザを目指す v1 には、取り込んだ PDF の任意の内容を描く処理が要る。
- SVG の図版と PDF の図版の区別。区別が要るときは、Golem の出力に印を足すか、別の経路で区別を渡す必要があり、Golem の側と合わせて決める。
- 箱の中に、画像や図版の名前、大きさ、ハッシュなどの文字を描くこと。

## テスト設計

- `q 100 0 0 50 10 20 cm /Im1 Do Q` を 72 dpi で描くと、`(10, 20)` から幅 100、高さ 50 の矩形（y を反転した装置の空間の位置）が `(192, 192, 192)` で塗られ、二本の対角線が `(96, 96, 96)` で描かれる。
- 90° 回した CTM で `Do` を呼ぶと、回った位置に箱が描かれる。
- `BBox [0 0 200 100]`、`Matrix [0.005 0 0 0.01 0 0]` の Form を `q 100 0 0 50 10 20 cm` で呼ぶと、上の画像と同じ箱になる。
- `Matrix` のない Form では、`BBox` を CTM で写した箱になる。
- ページの外にはみ出す箱は、ページの縁で切れる。
- 幅 0 の行列（`0 0 0 50 10 20 cm`）の `Do` は何も描かず、エラーにもならない。
- 画像のストリームが壊れた Flate でも、未知のフィルタでも、エラーにならない（読まないので）。
- `Subtype /PS` の XObject、`BBox` のない Form、資源にない名前の `Do` は、エラーになる。
- 箱を描いた後の `re f` は、箱の前に指定した色で塗られる。
- Golem の `figures` の fixture（SVG、PDF、JPEG の図版を含む）を描いたゴールデンを置く。

## トレードオフ

- 箱は、画像と図版の中身の変化を見つけない。中身の変化は、Golem の出力のゴールデンのハッシュと本文で見つける。ただし、再圧縮した PNG は、Golem の側の fixture の追加が済むまで、どちらでも見つからない。
- 箱は不透明に塗るので、透明な画像の下にある描画の変化は、箱の範囲では見つからない。Golem のページで、画像の下に描くものは少ないと見込む。
- 画像と図版を描くようになると、同じ入力の画素が大きく変わる。[ADR 0001](0001-scope-golem-contract-and-determinism.md) のとおりマイナーバージョンを上げ、Golem は参照の画像を作り直す。
- 対角線の幅を CTM によらず 1 ポイントにしたので、小さな図版では箱がほとんど対角線で埋まる。箱の位置と大きさがわかれば目的に足りる。

## 参考文献

- ISO 32000-1:2008 https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf ：8.8（外部のオブジェクト）、8.9.4（画像の空間）、8.10（Form XObject）
- Golem の事実の調査：[Writer.hs:505-540](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L505-L540)、[Writer.hs:648-685](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-pdf/src/Golem/Pdf/Writer.hs#L648-L685)、[OutputGoldenSpec.hs](https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-driver/test/OutputGoldenSpec.hs)
