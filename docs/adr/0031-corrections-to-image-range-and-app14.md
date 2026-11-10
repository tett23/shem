# ADR 0031: 画像の座標の範囲のエラーと、成分 1 の JPEG の APP14 の扱いの修正

ステータス: 採択

## 文脈

[ADR 0027](0027-images-and-figures.md) から [ADR 0030](0030-limits-on-forms-clips-and-jpeg-scans.md) の実装のテストを点検したところ（2026-11-10）、ADR の文と実装の振る舞いが違う箇所が二つ見つかった。
どちらも実装の振る舞いは安全で、ADR の文のほうが不正確だった。

- [ADR 0029](0029-details-left-open-by-images-and-forms.md) は、「CTM の成分が 2^57 を超える画像は、`image outside the range of coordinates` のエラーにする」とする。`Do` で画像を置くと、その前に画像の角を装置の空間に写す計算が失敗し、`arithmetic overflow` のエラーになる。2^57 の検査は、画像の写像を作る関数の中にあるが、`Do` からは届かない。
- [ADR 0028](0028-details-left-open-by-images-and-figures.md) は、「成分 1 では `transform` を見ない」とする。実装は、APP14 の `transform` が 2 を超えれば、成分の数によらず `unsupported JPEG APP14 transform <t>` のエラーにする。成分 1 で見ないのは、0、1、2 の値だけである。

## 実装すること

- `Do` で置く画像の CTM が大きすぎて、画像の角を装置の空間に写せなければ、`arithmetic overflow` のエラーにする（今の振る舞いのとおり）。写像を作る関数の 2^57 の検査は、写像の計算の前提を守るために残す。
- APP14 の `transform` が 2 を超えれば、成分の数によらずエラーにする。成分 1 では、`transform` が 0、1、2 のどれでも、変換をしない（今の振る舞いのとおり）。
- 仕様の該当する記述を、この振る舞いに合わせる。

## 実装しないこと

- コードの振る舞いを変えること。

## テスト設計

- `Do` で大きすぎる CTM の画像を置くと、`arithmetic overflow` のエラーになる（`src/render/image_tests.rs` にある）。
- 写像を作る関数の 2^57 の検査は、単体テストで確かめる（`src/picture/tests.rs` にある）。
- 成分 1 の JPEG で、`transform` が 0、1、2 なら変換をせず、7 ならエラーになる（`src/jpeg/tests.rs` にある）。

## トレードオフ

- エラーの文が、座標の範囲の文ではなく `arithmetic overflow` になる。どちらも描く前の検査で出るので、出力は書かれない。

## 参考文献

- [ADR 0028](0028-details-left-open-by-images-and-figures.md)
- [ADR 0029](0029-details-left-open-by-images-and-forms.md)
