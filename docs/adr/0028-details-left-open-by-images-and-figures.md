# ADR 0028: 画像と図版の ADR が決めていなかった細部

ステータス: 採択

## 文脈

[ADR 0027](0027-images-and-figures.md) の実装（2026-11-10、Fable のエージェントが三つに分けて行った）で、[ADR 0027](0027-images-and-figures.md) に書かれていない振る舞いと、書かれたとおりには実装できなかった振る舞いが見つかった。
実装は、それぞれ次のように決めた。
この ADR は、その決定を記録する。

## 実装すること

### JPEG

- **色差の補間の条件：** 2×1 と 2×2 の fancy upsampling は、libjpeg の `jdsample.c` と同じく、間引いた成分の幅が 2 より大きいときだけ行い、ほかは標本を繰り返す。[ADR 0027](0027-images-and-figures.md) はこの条件を書いていない。条件を入れないと、Golem の `photo.jpg`（色差の幅が 1）が djpeg と pdftoppm に合わない。
- **1×2（4:4:0）：** [ADR 0027](0027-images-and-figures.md) のとおり標本を繰り返す。libjpeg-turbo と pdftoppm は補間するので、標本が最大 60 違う。
- **色空間の推定：** [ADR 0027](0027-images-and-figures.md) の規則（APP14 を先に見て、なければ成分 3 を YCbCr）に従う。libjpeg は、JFIF のマーカーを APP14 より優先し、成分の ID が `R`、`G`、`B` なら RGB とするが、それはしない。
- **APP14 の値：** `transform` が 2 より大きいもの、成分 4 の 1、成分 3 の 2 は、`unsupported JPEG APP14 transform <t>` のエラーにする。成分 1 では `transform` を見ない。
- **逆 DCT：** 中間値は i64 で計算する。12 ビットの壊れた入力では、i32 では足りない。結果は 0 から `2^精度 − 1` に切り詰める（libjpeg の表は、範囲から大きく外れた値を折り返す）。逆量子化の積は i32 に飽和させる。
- **順次の符号化の走査：** `Ss`、`Se`、`Ah`、`Al` が 0、63、0、0 でなければエラーにする。プログレッシブの JPEG が、一部の走査の後に EOI で終われば、残りの係数を 0 として復号する。EOI のないまま終わればエラーにする。
- **エラーの文：** `unsupported JPEG SOF<n>`、`unsupported JPEG precision <p>`、`corrupt JPEG data: <内容>` などとする。

### 線

- **一般の CTM での破線の周期：** 周期が、CTM が最も伸ばす向きでも 1/16 画素未満なら、`dash pattern shorter than 1/16 pixel` のエラーにする。
- **利用者の空間の座標：** 角を保たない CTM での線は、利用者の空間の点の絶対値が 2^30 ポイント以上なら、`stroke outside the range of coordinates` のエラーにする。点の差を i64 に収めるためである。
- **利用者の空間の方向：** 法線と継ぎ目に使う方向は、[ADR 0023](0023-miter-points-within-the-limit.md) と同じく 2^28 未満にシフトする。
- **`2 J` の長さ 0 の破線：** 何も描かない。向きがないからである。
- **`1 j` の折り返し：** 180 度で折り返す頂点にも、円板を加える。
- **`2 Tr` の字の線と箱：** butt の線端と上限 10 の miter のまま描き、角を保たない CTM ではエラーにする。切り抜きは当てる。
- **`W` の置き場所のエラー：** 現在の点がなければ `W without a current point`、`W` とパスを終える演算子の間にほかの演算子があれば `<演算子> after W` とする。
- **色のエラー：** `unsupported ICCBased with N <n>`、`malformed ICCBased color space`、`sc with <n> components for a color space of <m>` などとする。

## 実装しないこと

- libjpeg との違い（1×2 の補間、JFIF のマーカーの優先、成分の ID での RGB の推定）を埋めること。必要になれば、別の ADR で変える。

## テスト設計

- 上の決定は、実装の単体テストで確かめる（`src/jpeg/tests.rs`、`src/render/affine_tests.rs` など）。
- 1×2 の JPEG は、djpeg との違いを数値で固定するテストを置く。

## トレードオフ

- libjpeg と違う振る舞いを残すので、1×2 の JPEG は pdftoppm と見た目が少し違う。Golem の fixture には 1×2 の JPEG はない。

## 参考文献

- [ADR 0027](0027-images-and-figures.md)
- Independent JPEG Group の libjpeg（`jdsample.c`、`jdapimin.c`）https://www.ijg.org/
