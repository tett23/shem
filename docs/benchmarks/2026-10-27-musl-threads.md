# musl の配布物のスレッドの時間（[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md)）

[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) の「測定」の節が求める、変更の前と後の記録である。
v0.1.0-rc2 と、[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) を実装した後の shem で、Golem の『蟹工船』の fixture を `--jobs` を変えて描き、時間を比べた。
2026-10-27 に作り、コミットした後は変更しない。

## 対象

- PDF：`tests/fixtures/golem/kanikosen.pdf`（129 ページ、縦組み、1244995 バイト、SHA-256 は `6dc0d3fdff93f89c436a3764be91e0385993d740c5b2c3198e6ced5918fc192c`）。
- 解像度は 72 dpi、形式は PPM（Golem の CI と同じ）。
- 変更の前：v0.1.0-rc2（`152858295b7949514e05d0ae009e858a1262dfa0`）。
- 変更の後：GitHub Actions では `1c7910aee4fc4d6a6bbdf37e7b4bdac8ada851bb`（実装をマージした main の `dd308cd` に、ワークフローのログの直しだけを足したもの）、手元では実装のブランチの作業木（`dd308cd` と同じソース）。

## GitHub Actions（x86-64、4 vCPU）

- 計算機：ubuntu-24.04 のランナー、AMD EPYC 7763、4 プロセッサ。
- ビルド：x86_64-unknown-linux-musl を、リリースと同じく `cargo build --release --locked --target x86_64-unknown-linux-musl`（stable）で作ったもの。
- 方法：手動のワークフロー Benchmark（[実行 40499763067](https://github.com/tett23/shem/actions/runs/40499763067)、入力は `v0.1.0-rc2 HEAD`）。`scripts/ci/benchmark.sh` が、二つのビルドと `--jobs` の値を交互に 3 回ずつ走らせ、`date +%s%N` の差を記録した。

| ビルド | `--jobs` | 1 回目（ミリ秒） | 2 回目（ミリ秒） | 3 回目（ミリ秒） | 中央値（ミリ秒） |
|---|---|---|---|---|---|
| v0.1.0-rc2 | 1 | 3416 | 3413 | 3437 | 3416 |
| v0.1.0-rc2 | 2 | 4894 | 4888 | 5027 | 4894 |
| v0.1.0-rc2 | 4 | 11089 | 11304 | 11989 | 11304 |
| [ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) | 1 | 1414 | 1388 | 1398 | 1398 |
| [ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) | 2 | 759 | 754 | 751 | 754 |
| [ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) | 4 | 612 | 610 | 608 | 610 |

- v0.1.0-rc2 では、`--jobs 4` が `--jobs 1` の 3.3 倍の時間かかった。
- [ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) の後は、`--jobs 4` が `--jobs 1` の 0.44 倍の時間になり、ADR の目標（半分以下）に届いた。
- `--jobs 4` どうしでは、[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) の後が v0.1.0-rc2 の 18.5 倍の速さだった。`--jobs 1` どうしでも 2.4 倍の速さだった。
- すべての実行の出力のファイルは同じだった（`scripts/ci/benchmark.sh` が `diff -r` で確かめる）。
- この実行の前に、同じワークフローを main の `dd308cd` で一度走らせ（[実行 40499478650](https://github.com/tett23/shem/actions/runs/40499478650)）、中央値は v0.1.0-rc2 が 3400、4927、11738 ミリ秒、[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) が 1418、759、610 ミリ秒だった。そのときのワークフローは CPU の型番をログに出さなかったので、表には入れていない。

## 手元の Linux（colima）

- 計算機：Apple M1 Max の上の colima 0.10.3 の VM（aarch64 の Linux、2 CPU、メモリ 2GB）。
- ビルド：rust:1.99.1-slim-bookworm のコンテナで、`cargo build --release --locked` を `aarch64-unknown-linux-musl` と glibc（`aarch64-unknown-linux-gnu`、動的リンク）で行った。
- 方法：debian:stable-slim のコンテナで、`shem render --dpi 72 --format ppm --jobs <N>` を、四つのビルドと `--jobs` の値を交互に 3 回ずつ走らせ、`date +%s%N` の差を記録した。

| ビルド | `--jobs 1`（ミリ秒） | `--jobs 2`（ミリ秒） |
|---|---|---|
| musl、v0.1.0-rc2 | 2335、2323、2399（中央値 2335） | 3121、3502、3278（中央値 3278） |
| musl、[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) | 1233、1210、1178（中央値 1210） | 690、704、685（中央値 690） |
| glibc、v0.1.0-rc2 | 2122、2134、2246（中央値 2134） | 1224、1195、1243（中央値 1224） |
| glibc、[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) | 1140、1247、1255（中央値 1247） | 679、673、712（中央値 679） |

- musl のビルドは、v0.1.0-rc2 では `--jobs 2` が `--jobs 1` より 1.4 倍遅かった。[ADR 0013](../adr/0013-fewer-allocations-for-musl-threads.md) の後は 1.75 倍速く、glibc（1.84 倍）に近づいた。
- すべての出力のディレクトリの 129 個のファイルは同じで、`tests/fixtures/golem/kanikosen.sha256` の 72 dpi の PPM の一覧とも一致した。
