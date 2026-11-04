# PNG の書き出し、描画、Flate の展開の高速化の時間（[ADR 0015](../adr/0015-faster-png-render-and-decoding.md)）

[ADR 0015](../adr/0015-faster-png-render-and-decoding.md) の「測定と文書」の節が求める、変更の前と後の記録である。
v0.1.0-rc3、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) の後の main、[ADR 0015](../adr/0015-faster-png-render-and-decoding.md) を実装した shem で、Golem の『蟹工船』の fixture を `--jobs` を変えて描き、時間を比べた。
2026-11-05 に作り、コミットした後は変更しない。

## 対象

- PDF：`tests/fixtures/golem/kanikosen.pdf`（129 ページ、縦組み、SHA-256 は `6dc0d3fdff93f89c436a3764be91e0385993d740c5b2c3198e6ced5918fc192c`）。
- 画像：72 dpi の PPM（Golem の CI と同じ）と、150 dpi の PNG（既定の解像度と形式）。
- v0.1.0-rc3：`6c93aa9702e713e5d1af963d04e9a1b453bb3975`。
- main：[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) の後の `a8bbd4a00660f514128f74b45578158039176566`。
- [ADR 0015](../adr/0015-faster-png-render-and-decoding.md)：実装のブランチ perf/limit の `a28a83997377b091738d1182cac9dfc3dd09e294`。手元の測定も同じソースである。

## GitHub Actions（x86-64、4 vCPU）

- 計算機：二つの実行とも ubuntu-24.04 のランナーで、Intel Xeon 6973P-C、4 プロセッサ。`lscpu` は、コアあたり 2 スレッド、2 コア、1 ソケットだった。4 プロセッサは、2 コアのハイパースレッドである。
- ビルド：x86_64-unknown-linux-musl を、リリースと同じく `cargo build --release --locked --target x86_64-unknown-linux-musl`（stable）で作ったもの。
- 方法：手動のワークフロー Benchmark（入力は `v0.1.0-rc3 origin/main HEAD`）。`scripts/ci/benchmark.sh` が、三つのビルドと `--jobs` の値を交互に 3 回ずつ走らせた。表は 3 回の中央値（ミリ秒）で、括弧の中は 3 回の値である。どの実行の出力のファイルも同じだった。

### 72 dpi の PPM

[実行 41929265662](https://github.com/tett23/shem/actions/runs/41929265662)。

| ビルド | `--jobs 1` | `--jobs 2` | `--jobs 4` |
|---|---|---|---|
| v0.1.0-rc3 | 959（959、956、1025） | 540（540、537、583） | 420（414、420、450） |
| main | 585（570、585、622） | 329（327、329、330） | 248（246、294、248） |
| [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 358（356、383、358） | 214（209、231、214） | 163（163、175、163） |

### 150 dpi の PNG

[実行 41929279202](https://github.com/tett23/shem/actions/runs/41929279202)。

| ビルド | `--jobs 1` | `--jobs 2` | `--jobs 4` |
|---|---|---|---|
| v0.1.0-rc3 | 5825（5825、5884、5781） | 2982（2921、2982、3011） | 2548（2542、2548、2561） |
| main | 5117（5104、5179、5117） | 2649（2672、2649、2629） | 2282（2282、2300、2277） |
| [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 2264（2331、2261、2264） | 1184（1213、1184、1173） | 947（949、911、947） |

- 72 dpi の PPM では、[ADR 0015](../adr/0015-faster-png-render-and-decoding.md) は main より `--jobs 1` で 1.63 倍、`--jobs 4` で 1.52 倍速かった。
- 150 dpi の PNG では、[ADR 0015](../adr/0015-faster-png-render-and-decoding.md) は main より `--jobs 1` で 2.26 倍、`--jobs 4` で 2.41 倍速かった。
- どのビルドでも、`--jobs 4` は `--jobs 2` から 1.16〜1.33 倍しか伸びなかった。2 コアのハイパースレッドで、同じコアの二つのスレッドが計算の部品を分け合うためと考えられる。

## 手元の Linux（colima、8 CPU）

- 計算機：Apple M1 Max の上の colima 0.10.3 の VM（aarch64 の Linux、8 CPU、メモリ 8GB）。
- ビルド：rust:1.99.1-slim-bookworm のコンテナで、`cargo build --release --locked --target aarch64-unknown-linux-musl` で作った。v0.1.0-rc3 は Releases の配布物を使った。
- 方法：debian:stable-slim のコンテナで、`shem render --dpi <N> --format <F> --jobs <J>` を、三つのビルドと `--jobs` の値を交互に 5 回ずつ走らせ、`date +%s%N` の差を記録した。表は 5 回の中央値（ミリ秒）である。

| 画像 | ビルド | `--jobs 1` | `--jobs 2` | `--jobs 4` | `--jobs 8` |
|---|---|---|---|---|---|
| 72 dpi の PPM | v0.1.0-rc3 | 980 | 532 | 315 | 237 |
| 72 dpi の PPM | main | 518 | 293 | 173 | 116 |
| 72 dpi の PPM | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 274 | 157 | 102 | 82 |
| 150 dpi の PNG | v0.1.0-rc3 | 5683 | 2971 | 1617 | 885 |
| 150 dpi の PNG | main | 5089 | 2663 | 1426 | 765 |
| 150 dpi の PNG | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 1133 | 605 | 338 | 206 |

5 回の値は次のとおりである（小さい順）。

| 画像 | ビルド | `--jobs` | 値（ミリ秒） |
|---|---|---|---|
| 72 dpi の PPM | v0.1.0-rc3 | 1 | 969、972、980、981、982 |
| 72 dpi の PPM | v0.1.0-rc3 | 2 | 529、531、532、534、535 |
| 72 dpi の PPM | v0.1.0-rc3 | 4 | 312、314、315、315、315 |
| 72 dpi の PPM | v0.1.0-rc3 | 8 | 235、235、237、240、240 |
| 72 dpi の PPM | main | 1 | 516、517、518、520、524 |
| 72 dpi の PPM | main | 2 | 289、290、293、293、293 |
| 72 dpi の PPM | main | 4 | 173、173、173、174、175 |
| 72 dpi の PPM | main | 8 | 115、116、116、116、118 |
| 72 dpi の PPM | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 1 | 272、272、274、274、277 |
| 72 dpi の PPM | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 2 | 154、156、157、159、160 |
| 72 dpi の PPM | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 4 | 101、102、102、102、103 |
| 72 dpi の PPM | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 8 | 82、82、82、83、84 |
| 150 dpi の PNG | v0.1.0-rc3 | 1 | 5664、5681、5683、5691、5716 |
| 150 dpi の PNG | v0.1.0-rc3 | 2 | 2961、2967、2971、2972、2984 |
| 150 dpi の PNG | v0.1.0-rc3 | 4 | 1585、1587、1617、1630、1631 |
| 150 dpi の PNG | v0.1.0-rc3 | 8 | 881、883、885、885、886 |
| 150 dpi の PNG | main | 1 | 5081、5082、5089、5090、5116 |
| 150 dpi の PNG | main | 2 | 2663、2663、2663、2664、2683 |
| 150 dpi の PNG | main | 4 | 1422、1425、1426、1430、1435 |
| 150 dpi の PNG | main | 8 | 759、764、765、768、781 |
| 150 dpi の PNG | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 1 | 1126、1128、1133、1138、1152 |
| 150 dpi の PNG | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 2 | 603、605、605、607、608 |
| 150 dpi の PNG | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 4 | 338、338、338、339、341 |
| 150 dpi の PNG | [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) | 8 | 204、205、206、208、211 |

- 72 dpi の PPM の `--jobs 8` は、main の 1.41 倍、v0.1.0-rc3 の 2.9 倍の速さだった。
- 150 dpi の PNG の `--jobs 8` は、main の 3.7 倍、v0.1.0-rc3 の 4.3 倍の速さだった。
- [ADR 0015](../adr/0015-faster-png-render-and-decoding.md) の `--jobs 8` の出力は、72 dpi の PPM も 150 dpi の PNG も、v0.1.0-rc3 の `--jobs 1` の出力と同じだった（`diff -rq`）。
