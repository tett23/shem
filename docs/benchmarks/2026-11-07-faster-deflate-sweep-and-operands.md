# Deflate の探索、密な格子の掃き出し、演算子の数の高速化の時間（[ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md)）

[ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) の「測定と文書」の節が求める、変更の前と後の記録である。
v0.1.0-rc5（または main）と、[ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) を実装した shem で、Golem の『蟹工船』の fixture を描き、時間を比べた。
2026-11-07 に作り、コミットした後は変更しない。

## 対象

- PDF：`tests/fixtures/golem/kanikosen.pdf`（129 ページ、縦組み、SHA-256 は `6dc0d3fdff93f89c436a3764be91e0385993d740c5b2c3198e6ced5918fc192c`）。
- 画像：72 dpi の PPM、150 dpi の PNG（既定の解像度と形式）、300 dpi の PPM（ブラウザのプレイグラウンドの解像度）。
- v0.1.0-rc5：`333db8fcccfcd7f626d80f01a6ebb1029a5c6b44`。main（`f60c40cdc67871e17256a09437fd337ada078340`）とは文書だけが違う。
- [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md)：実装のブランチ impl/0019-faster-deflate-sweep-and-operands の `3679ae84c333d6e65051527eba93b6295cff9373`。

## GitHub Actions（x86-64、4 vCPU）

- 計算機：ubuntu-24.04 のランナー。72 dpi の PPM は AMD EPYC 9V45、150 dpi の PNG と 300 dpi の PPM は AMD EPYC 7763 で、どれも 4 プロセッサ（`lscpu` では、コアあたり 2 スレッド、2 コア、1 ソケット）だった。
- ビルド：x86_64-unknown-linux-musl を、リリースと同じく `cargo build --release --locked --target x86_64-unknown-linux-musl`（stable）で作ったもの。
- 方法：手動のワークフロー Benchmark（入力は `v0.1.0-rc5 HEAD`）。`scripts/ci/benchmark.sh` が、二つのビルドと `--jobs` の値を交互に 3 回ずつ走らせた。表は 3 回の中央値（ミリ秒）で、括弧の中は 3 回の値である。どの実行の出力のファイルも同じだった。

| 画像 | 実行 | ビルド | `--jobs 1` | `--jobs 2` | `--jobs 4` |
|---|---|---|---|---|---|
| 72 dpi の PPM | [42380628736](https://github.com/tett23/shem/actions/runs/42380628736) | v0.1.0-rc5 | 328（331、328、326） | 193（200、188、193） | 147（148、145、147） |
| 72 dpi の PPM | [42380628736](https://github.com/tett23/shem/actions/runs/42380628736) | [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) | 332（332、332、330） | 190（196、190、187） | 150（150、150、149） |
| 150 dpi の PNG | [42380631025](https://github.com/tett23/shem/actions/runs/42380631025) | v0.1.0-rc5 | 2962（2975、2961、2962） | 1530（1528、1538、1530） | 1250（1253、1236、1250） |
| 150 dpi の PNG | [42380631025](https://github.com/tett23/shem/actions/runs/42380631025) | [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) | 2011（2020、2011、1998） | 1049（1051、1049、1046） | 846（846、840、846） |
| 300 dpi の PPM | [42380633157](https://github.com/tett23/shem/actions/runs/42380633157) | v0.1.0-rc5 | 1731（1733、1716、1731） | 755（891、751、755） | 603（574、603、605） |
| 300 dpi の PPM | [42380633157](https://github.com/tett23/shem/actions/runs/42380633157) | [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) | 1298（1298、1301、1298） | 735（734、735、808） | 589（589、618、581） |

- 150 dpi の PNG では、`--jobs 1` で 1.47 倍、`--jobs 4` で 1.48 倍速かった。
- 300 dpi の PPM では、`--jobs 1` で 1.33 倍速かった。`--jobs 2` と `--jobs 4` の差は 3% 以下だった。
- 72 dpi の PPM では、差は測定のばらつきの中だった。

## 手元の Mac（Apple M1 Max、10 CPU）

- ビルド：`cargo build --release` で、main（`f60c40c`）と実装のブランチ（`3679ae8`）から作った。
- 方法：`shem render --dpi <N> --format <F> --jobs <J>` を、二つのビルドを交互に 5 回ずつ走らせ、`/usr/bin/time -l` の実時間を記録した。測定の間の負荷平均は 5.7〜7.5 だった。表は 5 回の中央値（秒）である。

| 画像 | ビルド | `--jobs 1` | `--jobs 10` |
|---|---|---|---|
| 72 dpi の PPM | main | 0.28（0.28、0.30、0.28、0.28、0.29） | 0.06（0.07、0.06、0.06、0.06、0.06） |
| 72 dpi の PPM | [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) | 0.28（0.28、0.29、0.28、0.27、0.28） | 0.06（0.07、0.06、0.06、0.06、0.06） |
| 150 dpi の PNG | main | 1.05（1.04、1.06、1.05、1.05、1.06） | 0.16（0.16、0.17、0.17、0.16、0.16） |
| 150 dpi の PNG | [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) | 0.96（0.96、0.96、0.96、0.96、0.97） | 0.15（0.16、0.15、0.15、0.15、0.16） |
| 300 dpi の PPM | main | 0.88（0.87、0.88、0.88、0.88、0.87） | 0.22（0.21、0.21、0.22、0.23、0.22） |
| 300 dpi の PPM | [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) | 0.82（0.82、0.82、0.83、0.82、0.83） | 0.22（0.22、0.22、0.23、0.21、0.23） |

- 150 dpi の PNG の `--jobs 1` は 9%、300 dpi の PPM の `--jobs 1` は 7% 短くなった。
- `--jobs 10` のユーザー時間は、150 dpi の PNG で 1.24 秒から 1.16 秒、300 dpi の PPM で 0.96 秒から 0.87 秒になった（5 回の中央値）。
- GitHub Actions の x86-64 より効果が小さい。

## wasm（Deno、手元の Mac）

- ビルド：`rustup run 1.99.1 sh scripts/ci/build-wasm.sh` で、main と実装のブランチから作った。大きさは 435,534 バイトと 411,579 バイトだった。
- 方法：Deno 2.9.7 で二つのモジュールを読み込み、kanikosen を 300 dpi で開いて最初の 20 ページを描く時間（開く時間を含む）を、交互に 5 回ずつ測った。

| ビルド | 時間（ミリ秒） | 中央値 |
|---|---|---|
| main | 303、268、263、263、267 | 267 |
| [ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) | 284、262、255、258、259 | 259 |

- 1 回目に文書を開く時間は、main で 27 ミリ秒、[ADR 0019](../adr/0019-faster-deflate-sweep-and-operands.md) で 19 ミリ秒だった。

## 出力

- fixture の五つの PDF を、72、97、150、300 dpi の PPM と 150、300 dpi の PNG で描いた 834 個のファイル、標準エラー、終了コードは、main と同じだった（手元の Mac）。
- `SHEM_WASM` に実装のブランチの wasm を、`SHEM_PPM` に同じブランチのネイティブの 144 dpi の PPM を与え、`wasm/test.ts` が通った。
