# ページの並列の描画の時間（[ADR 0009](../adr/0009-render-pages-in-parallel.md)）

[ADR 0009](../adr/0009-render-pages-in-parallel.md) の「性能」の節が求める計測の記録である。
Golem の『蟹工船』の fixture を組んだ PDF を、`--jobs 1` と `--jobs 10` で描き、時間を比べた。
2026-10-27 に作り、コミットした後は変更しない。

## 対象

- PDF：Golem の `modules/golem-driver/test/fixtures/kanikosen`（[Golem の 1254092a](https://github.com/tett23/golem/tree/1254092a2ed5a7fd29bb6f1affb53806cf76c694/modules/golem-driver/test/fixtures/kanikosen)）を、同じコミットの golem で `golem build --target pdf` して作ったもの。129 ページ、縦組み、1244995 バイト、SHA-256 は `6dc0d3fdff93f89c436a3764be91e0385993d740c5b2c3198e6ced5918fc192c`。
- 解像度は 150 dpi、形式は PNG。

## 環境

- 計算機：Apple M1 Max（10 コア）、メモリ 64GB、macOS 27.0。
- shem：impl/0009-parallel のブランチ（この記録を加えたコミットの親は `04a27dd`）を、`cargo build --release`（rustc 1.97.0-nightly）で作ったもの。
- 方法：毎回、空の出力のディレクトリに `shem render --dpi 150 --jobs <N> <PDF> <出力>` を実行し、`/usr/bin/time -p` の real を記録した。`--jobs 1` と `--jobs 10` を交互に 3 回ずつ走らせた。

## 結果

| 回 | `--jobs 1`（秒） | `--jobs 10`（秒） |
|---|---|---|
| 1 | 16.59 | 2.49 |
| 2 | 16.37 | 2.41 |
| 3 | 16.19 | 2.24 |
| 中央値 | 16.37 | 2.41 |

- `--jobs 10` は `--jobs 1` のおよそ 6.8 倍の速さだった。
- 二つの出力のディレクトリの 129 個のファイルは、`diff -r` で同じだった。
- この表の前に、計算機の負荷が高い間（load average 68）に一度ずつ測り、`--jobs 1` が 40.91 秒、`--jobs 10` が 4.80 秒だった。ほかの処理と重なった値なので、表には入れていない。
