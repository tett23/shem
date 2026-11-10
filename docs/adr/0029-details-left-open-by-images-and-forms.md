# ADR 0029: 画像の XObject と Form の、ADR が決めていなかった細部

ステータス: 採択

## 文脈

[ADR 0027](0027-images-and-figures.md) の画像の XObject と Form の実装（2026-11-10、Fable のエージェント）で、[ADR 0027](0027-images-and-figures.md) と [ADR 0028](0028-details-left-open-by-images-and-figures.md) に書かれていない振る舞いが見つかった。
実装は、それぞれ次のように決めた。
この ADR は、その決定を記録する。

## 実装すること

### 画像

- **`Indexed` の `Decode`：** 省略か `[0 hival]` だけを認め、ほかはエラーにする。[ADR 0027](0027-images-and-figures.md) の「既定か、すべての組を反転した配列」は、成分の値の配列を前提にしていて、番号の反転には意味がない。
- **`Indexed` の標本の数：** `Width × Height × 成分の数` の上限（2^26）では、`Indexed` の成分の数を 1（番号）と数える。
- **16 ビットの画像の伸長：** 伸長の上限は、画像の大きさから決まるバイト数とする。2^26 の標本の 16 ビットの画像は 2^27 バイトになり、[ADR 0021](0021-limits-on-hostile-input.md) のストリームの上限（2^26 バイト）を超えるが、画像の大きさで抑える。
- **不透明度の積の順：** 覆いと `SMask` の不透明度を先に掛け、その後で切り抜きを掛ける。`div_round(x × y, 255)` は結合的でないので、順で値が 1 変わりうる。
- **透明な標本の補間：** [ADR 0027](0027-images-and-figures.md) の式のとおり、RGB と不透明度を別々に補間する（乗算済みの値では補間しない）。完全に透明な標本の色が、隣の画素ににじむ。
- **面積のない配置：** CTM の行列式が 0 の画像と Form は、何も描かず、エラーにしない（[ADR 0006](0006-placeholder-boxes-for-images-and-forms.md) の箱と同じ）。
- **座標の範囲：** CTM の成分が 2^57 を超える画像は、`image outside the range of coordinates` のエラーにする。
- **`Indexed` の `hival`：** 0 から 255 に限り、ほかは `malformed hival of an Indexed color space` のエラーにする（ISO 32000-1 の 8.6.6.3 の範囲）。
- **JPEG の標本化係数：** どの成分の係数も、成分の中の最大の係数を割り切らなければ、`unsupported JPEG sampling factors that do not divide the largest` のエラーにする。
- **JPEG の DNL：** 高さが 0 の SOF（高さを DNL のマーカーで後から決めるもの）は、`unsupported JPEG frame with height 0 (DNL)` のエラーにする。
- **キャッシュの大きさ：** 復号した画像のキャッシュは 2^28 バイトまでとし、超えた画像は `Do` のたびに復号する。出力は変わらない。

### Form

- **エラーの位置：** Form の内容の中のエラーは、Form のオブジェクトの番号と、Form の内容の中のバイトの位置で示す（たとえば `page 1: object 5: offset 21: unsupported operator RG`）。
- **面積のない `BBox`：** 面積が 0 の `BBox` の Form は、切り抜きが空になるので、何も描かない。エラーにはしない。
- **文字の状態の演算子：** Form の中でエラーにするのは `BT` だけで、`Tf` などの文字の状態の演算子は、`BT` の外では今のとおり認める。

### エラーの文

- `forms nested deeper than 8`、`text in a form`、`a form without Resources`、`a form without a BBox`、`unsupported Group <値>` などの Form のエラーと、`unsupported <キー> <値>`、`unsupported Decode [..]`、`index <i> out of range of an Indexed color space of hival <h>`、`<キー> <値> of the image does not match the JPEG frame (<値>)` などの画像のエラーは、実装の文のとおりとする。

### fixture

- `figures.pdf` は、今の Golem で作りなおすと 9 ページになる。`complex-horizontal.pdf` と `complex-vertical.pdf` は白紙の偶数ページを持つので、fixture の検査は、この二つに限り白紙のページを認める。
- `complex-horizontal` と `complex-vertical` のゴールデンは、画像ではなく SHA-256 の一覧で持つ（kanikosen と同じ）。

## 実装しないこと

- 乗算済みの値での補間。透明な標本の色のにじみをなくすには、別の ADR で補間の式を変える。

## テスト設計

- 上の決定は、実装の単体テストで確かめる（`src/picture/tests.rs`、`src/render/image_tests.rs`、`src/render/form_tests.rs` など）。

## トレードオフ

- 透明な標本の色がにじむので、`SMask` や tRNS の画像を拡大すると、透明な部分の縁に色が出る。Golem の `figures` の 6 ページ目と 8 ページ目に見える。

## 参考文献

- [ADR 0027](0027-images-and-figures.md)
- [ADR 0028](0028-details-left-open-by-images-and-figures.md)
