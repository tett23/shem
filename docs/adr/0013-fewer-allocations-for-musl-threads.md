# ADR 0013: 細かいメモリ割り当てを減らし、musl の配布物でもスレッドで速くする

ステータス: 採択（未実装）

## 文脈

Golem の CI で、shem v0.1.0-rc2 の `*-unknown-linux-musl` の配布物は、`--jobs` を 2 以上にすると `--jobs 1` より大幅に遅くなることがわかった（Golem のセッションからの依頼、2026-10-27）。
macOS の配布物は、スレッドを増やすと速くなる。

Golem が GitHub Actions の ubuntu-24.04（x86-64、4 vCPU）で、使い手のマニュアル（310 ページ）を 72 dpi の PPM で描いて測った値は次のとおりである。

| `--jobs` | 実時間 | user | sys |
|---|---|---|---|
| 1 | 8.39 秒 | 7.64 | 0.74 |
| 2 | 20.06 秒 | 20.84 | 13.06 |
| 4 | 57.34 秒 | 54.62 | 96.27 |

`strace -f -c` では、`--jobs 4` で futex が約 383 万回に増え、syscall の時間の 91% を占めた。
mmap と munmap は、`--jobs 1` で約 2 万 9 千回ずつ（1 ページあたり約 95 回）だった。
Golem は、shem を PDF ごとに `--jobs 1` で起動し、そのプロセスを並べる形にして回避している（[Golem の #434](https://github.com/tett23/golem/pull/434)、[#437](https://github.com/tett23/golem/pull/437)）。

### 手元での再現と切り分け

手元の colima（aarch64 の Linux、2 CPU）で、Golem の `kanikosen` の fixture を組んだ PDF（129 ページ、[2026-10-27-parallel-render.md](../benchmarks/2026-10-27-parallel-render.md) と同じもの）を 72 dpi の PPM で描いた。
同じ rc2 のソースを、musl、glibc（動的リンク）、glibc（`-C target-feature=+crt-static` で静的リンク）の三通りにビルドして比べた（3 回の中央値）。

| ビルド | `--jobs 1` | `--jobs 2` |
|---|---|---|
| musl（rc2 の配布物） | 2347 ミリ秒 | 3243 ミリ秒 |
| glibc（動的リンク） | 2175 ミリ秒 | 1203 ミリ秒 |
| glibc（静的リンク） | 2043 ミリ秒 | 1123 ミリ秒 |

- 同じソースで glibc なら 2 スレッドでほぼ倍の速さになるので、shem 自身のロック（[ADR 0009](0009-render-pages-in-parallel.md) の失敗の記録、[ADR 0011](0011-speed-up-without-changing-bytes.md) の `FontCache`）は原因ではない。
- 三つのビルドの出力は、すべて同じバイト列だった。

musl のメモリ割り当て（mallocng）は、一つの錠 `__malloc_lock` で守られている（[glue.h:59-87](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/glue.h#L59-L87)）。
読むための錠も排他（`RDLOCK_IS_EXCLUSIVE`）で、`malloc` は呼ぶたびにこの錠を取る（[malloc.c:337](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/malloc.c#L337)）。
131052 バイト以上の割り当ては、mmap で直接取る（[meta.h:12](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/meta.h#L12)、[malloc.c:308](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/malloc.c#L308)）。
したがって、細かい割り当てが多いほど、スレッドどうしがこの錠を奪い合い、futex が増える。

### 割り当ての回数

数えるための全体のメモリ割り当て（`System` を包み、回数を数えるもの）をかぶせた作業用のプログラムで、同じ PDF の 1 ページあたりの割り当てを数えた。
1 ページに描くグリフは約 500 個である。

| 段階 | alloc | realloc | 131052 バイト以上 |
|---|---|---|---|
| 検査（`check`） | 36,269 | 16,918 | 2.0 |
| 描画（`render`） | 159,949 | 110,434 | 2.8 |
| PPM への書き出し | 2 | 0 | 1 |
| PNG への書き出し | 527 | 28 | 7.1 |

描画では、1 グリフあたり約 530 回の割り当てと realloc がある。
コードを読むと、主な出どころは次のとおりである。

- **線分ごと**：ラスタライザの `render_line` が、線分ごとに行と列の交点の `Vec` を 3〜4 個作る（[src/raster.rs:169-178](https://github.com/tett23/shem/blob/1dbb0bc6086aa676a26ce17b5a3c0bf522ec6caa/src/raster.rs#L169-L178)、[src/raster.rs:190-230](https://github.com/tett23/shem/blob/1dbb0bc6086aa676a26ce17b5a3c0bf522ec6caa/src/raster.rs#L190-L230)）。
- **曲線ごと**：`flatten_curve` が、曲線ごとに結果と分割を待つ曲線の `Vec` を作る（[src/raster.rs:124-141](https://github.com/tett23/shem/blob/1dbb0bc6086aa676a26ce17b5a3c0bf522ec6caa/src/raster.rs#L124-L141)）。
- **塗りごと**：`coverage` が、セル、合わせたセル、行ごとのセル、覆いの `Vec` を作る（[src/raster.rs:151-165](https://github.com/tett23/shem/blob/1dbb0bc6086aa676a26ce17b5a3c0bf522ec6caa/src/raster.rs#L151-L165)、[src/raster.rs:248-310](https://github.com/tett23/shem/blob/1dbb0bc6086aa676a26ce17b5a3c0bf522ec6caa/src/raster.rs#L248-L310)）。
- **グリフごと**：グリフのパスを部分パスごとの `Vec` で作る（[src/raster.rs:32-78](https://github.com/tett23/shem/blob/1dbb0bc6086aa676a26ce17b5a3c0bf522ec6caa/src/raster.rs#L32-L78)、[src/render.rs:197](https://github.com/tett23/shem/blob/1dbb0bc6086aa676a26ce17b5a3c0bf522ec6caa/src/render.rs#L197)）。合成の太字の線は、多角形ごとに `Vec` を作る。
- **字句ごと**：内容のストリームの字句が、演算子のキーワードと名前を `Vec<u8>` に写す。検査と描画で二回解釈するので、二回ずつ起きる。

### 試作での確かめ

出力を変えずに割り当てを減らす試作を作り、同じ環境で測った（3 回の中央値）。
試作は二段階で、一段目は `render_line` の交点をイテレーターで流し、`flatten_curve` の分割を待つ曲線を固定の長さの配列に積むものである。二段目は、塗りのセルと覆いの `Vec` をページの間で使い回し、同じセルの足し合わせを `dedup_by`、行の取り出しを `chunk_by` でその場で行うものである。

| ビルド | 描画の alloc／realloc（1 ページ） | `--jobs 1` | `--jobs 2` |
|---|---|---|---|
| musl、rc2 | 159,949／110,434 | 2347 ミリ秒 | 3243 ミリ秒 |
| musl、試作の一段目 | 17,290／13,508 | 1453 ミリ秒 | 1045 ミリ秒 |
| musl、試作の二段目 | 11,981／5,465 | 1332 ミリ秒 | 890 ミリ秒 |
| glibc、rc2 | 同上 | 2175 ミリ秒 | 1220 ミリ秒 |
| glibc、試作の二段目 | 同上 | 1266 ミリ秒 | 728 ミリ秒 |

- 試作のどちらの段階も、出力は rc2 とバイト単位で同じで、`tests/golden/` のゴールデンも作り直さずに通った。
- 割り当てを減らすと、musl でも 2 スレッドで速くなり、一つのスレッドでも glibc でも速くなった。
- ただし、musl の伸び（1.5 倍）は glibc（1.74 倍）に届いていない。残りの割り当ては、主にグリフのパスと字句で、検査と描画を合わせて 1 ページ約 2 万 5 千回ある。
- 手元の VM は 2 CPU なので、4 コアでの効き目は測れていない。

## 実装すること

### 原則

- この ADR の変更は、どれも画素と PNG と PPM のバイト列を変えない。[ADR 0011](0011-speed-up-without-changing-bytes.md) と同じく、`tests/golden/` のゴールデンを作り直さずに通すことを条件とする。
- 依存は増やさない。配布物のターゲット（[ADR 0001](0001-scope-golem-contract-and-determinism.md) の musl）も変えない。
- 目標は、ページを描く間の割り当てを、線分、曲線、行、字句の数に比例しないようにすることである。割り当ては、ページかグリフの単位で定数回に抑える。

### ラスタライザ

- `render_line` の行と列の交点を、`Vec` に集めずにイテレーターで順に流す。交点の計算の式と順は変えない。
- `flatten_curve` の分割を待つ曲線を、長さ MAX_DEPTH + 1（17）の配列に積む。分割は一つの曲線を取って一段深い二つを積むので、この長さを超えない。点は、結果の `Vec` を作らずに、呼び出し元のパスに直接足す。
- 塗りの作業用の場所（`Scratch`：セルと覆いの `Vec`）を設け、一つのページを描く間は使い回す。同じセルの足し合わせはその場で行い（並べ替えてから `dedup_by`）、行は `chunk_by` のスライスとして取り出す。
- `coverage` の形（パスから覆いの `Vec` を返す関数）は、テストと参照の実装のために残す。

### パス

- `Path` を、すべての点を入れた一つの `Vec` と、部分パスの範囲と閉じているかの一覧で表す。部分パスごとの `Vec` をやめる。
- 描画は、グリフのパスと線の多角形のための `Path` を一つずつ持ち、使うたびに中身だけを空にして使い回す（容量は残す）。
- 線（[ADR 0003](0003-paths-fill-stroke-and-antialiasing.md)）は、多角形を出力の `Path` に直接足す。多角形ごとの `Vec` をやめる。

### 内容のストリームの字句

- 字句のキーワードと名前を `Vec<u8>` に写さず、元のバイト列のスライスとして扱う。演算子の表（[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md)）との照合も、スライスどうしで行う。
- 文字列（`Tj` の被演算子）は、エスケープを解くので、今と同じく `Vec<u8>` にしてよい。

### PPM の書き出し

- `shem render` の PPM は、ヘッダーと画素を、間に別の `Vec` を作らずにファイルへ書く。ページごとの 131052 バイト以上の割り当てを一つ減らす。`encode_ppm` の形は残す。

### 割り当ての回数の検査

- 結合テストに、数えるための全体のメモリ割り当て（`System` を包み、回数を数えるもの）を置く。結合テストは一つずつ別の実行ファイルになるので、ほかのテストには影響しない。
- 同じグリフを 1 個描くページと 101 個描くページを、`Session` で検査して描き、割り当ての回数の差が 100 × 4 以下であることを確かめる（グリフあたり 4 回まで）。CFF（Golem の fixture の Noto Serif JP）と TrueType の両方で確かめる。
- 同じ線分の数の直線を 1 本描くページと 100 本描くページ、曲線の数の違うパスを塗るページでも、差が線分と曲線の数に比例しないことを確かめる。

### 測定

- 速さの測定のために、`kanikosen` の PDF を fixture（`tests/fixtures/golem/kanikosen.pdf`）に加える。作り方を `tests/fixtures/golem/make.sh` と fixture の README に加える。Noto Serif JP は OFL で、ライセンスの文書はすでに置いてある。
- GitHub Actions の手動で起動するワークフロー（`workflow_dispatch`）として、x86-64 の musl の配布物と同じビルドで、`kanikosen` を 72 dpi の PPM で `--jobs 1`、`2`、`4` で 3 回ずつ描いて時間を出すものを置く。ランナーは ubuntu-24.04（4 vCPU）とする。
- 変更の前と後を、手元の colima（2 CPU）と、このワークフローで測り、`docs/benchmarks/` に記録として残す。

### 目標と、届かないとき

- 目標は、ubuntu-24.04（4 vCPU）の x86-64 の musl のビルドで、`--jobs 4` の実時間が `--jobs 1` の半分以下になることとする。
- 目標に届かなくても、配布物は musl のままとする。残った割り当て（グリフのパスなど）をさらに減らす案を、測った値を添えて別の ADR で検討する。

### 版と文書

- 出力を変えないので、パッチバージョンを上げる（[ADR 0001](0001-scope-golem-contract-and-determinism.md)）。
- リリースノートに、musl の配布物で `--jobs` を増やすと遅くなる問題を直したこと（目標に届いたかどうか）を書く。Golem に結果を伝え、`--jobs` を使う形に戻すかは Golem の側で決める。

## 実装しないこと

- 依存としての別のメモリ割り当て（mimalloc など）と、自前の汎用のメモリ割り当て。依存を増やさない方針に反し、自前の割り当てを正しく速く作る手間は大きい。
- Linux の配布物のターゲットの変更（glibc への切り替え）。glibc を静的リンクした配布物は作らない（レビューでの判断）。
- Linux での `--jobs` の既定の値の変更。既定は使えるコアの数のまま（[ADR 0009](0009-render-pages-in-parallel.md)）とする。
- PNG の書き出しの大きな割り当て（フィルタ後の行と zlib の出力）の使い回し。1 ページあたり約 7 回で、ページの数に比例するだけである。使い回すには `Session::render_pages` の `consume` の形を変える必要がある。
- ページごとの画像（`Canvas`）の使い回し。`consume` に画像を渡す形（[ADR 0012](0012-parallel-library-api.md)）を保つ。
- 割り当ての回数を減らすための `unsafe` のコード。

## テスト設計

### 出力が変わらないこと

- 各変更の後に、既存のゴールデン（11 個の PNG）を作り直さずに通す。CI の三つの環境でも通す。
- `kanikosen` の fixture を変更の前と後の shem で描き（72 dpi と 150 dpi、PNG と PPM）、すべてのファイルが同じバイト列であることを確かめる。これは自動のテストにする（変更の前の出力の SHA-256 の一覧を、fixture と同じディレクトリに置いて比べる）。

### 変わった関数

- `flatten_curve` の結果が、今の実装（テストの中に参照の実装として残す）と、ランダムに近い 1000 本の曲線（固定の種、深さが 16 に達するものを含む）で同じである。
- 新しい `render_line` と `Scratch` を使う塗りの覆いが、今の `coverage` の実装（参照の実装として残す）と、ランダムに作った 200 本のパス（両方の塗りの規則）で同じである。同じ `Scratch` を続けて使っても、毎回同じになる。
- 新しい `Path` で作った線の多角形が、今の `stroke` の結果と、点の並びと向きまで同じである（既存の線のテストと、ランダムな折れ線 200 本）。
- 新しい字句が、既存の字句と内容のストリームのテストをすべて通し、Golem の fixture のすべてのページで今と同じ演算子の列を返す。

### 割り当ての回数

- 上の「割り当ての回数の検査」の結合テストを置く。

### 速さ

- 自動のテストにはしない。上の「測定」のとおり記録する。

## トレードオフ

- 割り当てを減らす方針にしたので、依存も配布物の形も変えずに済み、glibc と macOS の一つのスレッドでも速くなる。そのかわり、musl のメモリ割り当ての錠はなくならない。残った割り当ての分の奪い合いは、コアの数が多いほど効いてくる。4 コアでの効き目は、まだ測れていない。
- glibc の静的リンクに替える案は、手元の 2 CPU で最も伸びが大きく、すぐに効く。しかし、配布物の名前が変わって Golem が固定している取得の手順に影響する。また、glibc の静的リンクは推奨されていない（`getaddrinfo` などの警告が出る。shem は使わない）。LGPL の静的リンクの条件も確かめる必要がある。レビューで、glibc を静的リンクしないと決めたので、採らない。
- `Path` の表し方と字句の形を変えるので、内部の API とテストのコードの変更が広い。参照の実装と比べるテストで、結果が変わらないことを確かめる。
- 割り当ての回数を数えるテストは、標準ライブラリの内部の割り当ての数が Rust の版で変わると、値が揺れうる。差（101 個と 1 個の差）で確かめ、上限に余裕を持たせて、版による揺れを受けないようにする。

## 参考文献

- Golem のセッションからの依頼「shem への依頼：musl 版で `--jobs` を増やすと遅くなる」（2026-10-27）。Golem のリポジトリにはコミットされていない。
- [Golem の #434](https://github.com/tett23/golem/pull/434)、[#437](https://github.com/tett23/golem/pull/437)：shem を PDF ごとに `--jobs 1` で並べる回避
- musl の mallocng（GitHub の写し kraj/musl の bd54dc3e）：[glue.h:59-87](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/glue.h#L59-L87)（`__malloc_lock`、`RDLOCK_IS_EXCLUSIVE`）、[malloc.c:308-378](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/malloc.c#L308-L378)、[meta.h:12](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/meta.h#L12)（`MMAP_THRESHOLD`）
- Rust の `std::alloc::GlobalAlloc` と `#[global_allocator]` https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html
- Rust の `slice::chunk_by` https://doc.rust-lang.org/std/primitive.slice.html#method.chunk_by 、`Vec::dedup_by` https://doc.rust-lang.org/std/vec/struct.Vec.html#method.dedup_by
