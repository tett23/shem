# スレッドごとの作業用の場所と、検査と塗りの高速化の時間（[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md)）

[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) の「測定と文書」の節が求める、変更の前と後の記録である。
v0.1.0-rc3 と、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) を実装した shem で、Golem の『蟹工船』の fixture を `--jobs` を変えて描き、時間を比べた。
2026-11-05 に作り、コミットした後は変更しない。

## 対象

- PDF：`tests/fixtures/golem/kanikosen.pdf`（129 ページ、縦組み、SHA-256 は `6dc0d3fdff93f89c436a3764be91e0385993d740c5b2c3198e6ced5918fc192c`）。
- 解像度は 72 dpi、形式は PPM。
- 変更の前：v0.1.0-rc3（`6c93aa9702e713e5d1af963d04e9a1b453bb3975`）。
- 変更の後：[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) の実装のブランチ experiment/arena の `5816a953ab0e724031c9b05706c10e85916ec798`。

## GitHub Actions（x86-64、4 vCPU）

- 計算機：ubuntu-24.04 のランナー、AMD EPYC 7763、4 プロセッサ。
- ビルド：x86_64-unknown-linux-musl を、リリースと同じく `cargo build --release --locked --target x86_64-unknown-linux-musl`（stable）で作ったもの。
- 方法：手動のワークフロー Benchmark（[実行 41907168697](https://github.com/tett23/shem/actions/runs/41907168697)、入力は `v0.1.0-rc3 HEAD`）。`scripts/ci/benchmark.sh` が、二つのビルドと `--jobs` の値を交互に 3 回ずつ走らせた。

| ビルド | `--jobs` | 1 回目（ミリ秒） | 2 回目（ミリ秒） | 3 回目（ミリ秒） | 中央値（ミリ秒） |
|---|---|---|---|---|---|
| v0.1.0-rc3 | 1 | 1460 | 1450 | 1444 | 1450 |
| v0.1.0-rc3 | 2 | 790 | 791 | 804 | 791 |
| v0.1.0-rc3 | 4 | 639 | 634 | 626 | 634 |
| [ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 1 | 799 | 814 | 803 | 803 |
| [ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 2 | 449 | 456 | 451 | 451 |
| [ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 4 | 356 | 355 | 354 | 355 |

- `--jobs 4` は、v0.1.0-rc3 の 1.79 倍、`--jobs 1` は 1.81 倍の速さだった。
- すべての実行の出力のファイルは同じだった（`scripts/ci/benchmark.sh` が `diff -r` で確かめる）。
- v0.1.0-rc3 の値は、同じワークフローで測った 2026-10-27 の記録（[2026-10-27-musl-threads.md](2026-10-27-musl-threads.md)、`--jobs 1` が 1398 ミリ秒、`--jobs 4` が 610 ミリ秒）より 4% ほど遅い。ランナーの違いによる揺れと考えられるが、確かめていない。

## 手元の Linux（colima、8 CPU）

- 計算機：Apple M1 Max の上の colima 0.10.3 の VM（aarch64 の Linux、8 CPU、メモリ 8GB）。
- ビルド：rust:1.99.1-slim-bookworm のコンテナで、`cargo build --release --locked` を `aarch64-unknown-linux-musl` と glibc（`aarch64-unknown-linux-gnu`、動的リンク）で行った。v0.1.0-rc3 の musl は、Releases の配布物を使った。
- 方法：debian:stable-slim のコンテナで、`shem render --dpi 72 --format ppm --jobs <N>` を、四つのビルドと `--jobs` の値を交互に 5 回ずつ走らせ、`date +%s%N` の差を記録した。表は 5 回の中央値（ミリ秒）である。

| ビルド | `--jobs 1` | `--jobs 2` | `--jobs 4` | `--jobs 8` |
|---|---|---|---|---|
| musl、v0.1.0-rc3 | 1092 | 583 | 344 | 268 |
| musl、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 585 | 318 | 189 | 129 |
| glibc、v0.1.0-rc3 | 1049 | 544 | 300 | 191 |
| glibc、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 577 | 311 | 180 | 124 |

5 回の値は次のとおりである。

| ビルド | `--jobs` | 値（ミリ秒） |
|---|---|---|
| musl、v0.1.0-rc3 | 1 | 1067、1090、1092、1092、1105 |
| musl、v0.1.0-rc3 | 2 | 573、581、583、585、588 |
| musl、v0.1.0-rc3 | 4 | 335、341、344、350、353 |
| musl、v0.1.0-rc3 | 8 | 255、256、268、276、294 |
| musl、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 1 | 573、578、585、586、598 |
| musl、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 2 | 311、314、318、327、338 |
| musl、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 4 | 183、186、189、191、197 |
| musl、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 8 | 124、125、129、139、142 |
| glibc、v0.1.0-rc3 | 1 | 1032、1048、1049、1062、1063 |
| glibc、v0.1.0-rc3 | 2 | 539、541、544、550、550 |
| glibc、v0.1.0-rc3 | 4 | 298、299、300、300、305 |
| glibc、v0.1.0-rc3 | 8 | 186、189、191、200、202 |
| glibc、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 1 | 561、571、577、580、599 |
| glibc、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 2 | 304、309、311、311、315 |
| glibc、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 4 | 177、178、180、180、183 |
| glibc、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) | 8 | 118、123、124、125、129 |

- musl の `--jobs 8` は、v0.1.0-rc3 の 2.1 倍の速さになった。musl と glibc の差は、v0.1.0-rc3 の 40% から 4% に縮んだ。
- `--jobs 1` と `--jobs 8` で、[ADR 0014](../adr/0014-thread-workspaces-and-faster-check-and-fill.md) の musl の出力は v0.1.0-rc3 の musl の出力と同じだった（`diff -rq`）。
