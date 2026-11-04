# ADR 0014: スレッドごとの作業用の場所を持ち、検査と塗りを速くする

ステータス: 採択

## 文脈

[ADR 0013](0013-fewer-allocations-for-musl-threads.md) で細かい割り当てを減らした後も、musl の配布物は、スレッドを増やしたときの伸びが glibc より小さかった（2026-11-05 の依頼）。
依頼には、内部でアリーナを持つ案も添えられていた（捨ててもよい案として）。

### 手元での再現

手元の Mac（Apple M1 Max）に、8 CPU の colima の VM（aarch64 の Linux）を作り、`tests/fixtures/golem/kanikosen.pdf`（129 ページ）を 72 dpi の PPM で描いた（3 回の中央値）。
v0.1.0-rc3 の配布物（musl）と、同じソースの glibc のビルドを比べた。

| ビルド | `--jobs 1` | `--jobs 4` | `--jobs 8` |
|---|---|---|---|
| musl | 1091 ミリ秒 | 352 ミリ秒 | 268 ミリ秒 |
| glibc | 1061 ミリ秒 | 302 ミリ秒 | 186 ミリ秒 |

glibc の割り当ての設定を変えて、原因を分けた。

- `MALLOC_MMAP_THRESHOLD_=131072`（大きな割り当てをいつも mmap で取る）にしても、`--jobs 8` は 184 ミリ秒から 189 ミリ秒で、ほとんど変わらなかった。
- `MALLOC_ARENA_MAX=1`（錠を一つにする）にすると、`--jobs 8` は 236 ミリ秒になり、musl に近づいた。

したがって、残りの差は、まだ残っている割り当てで錠を奪い合うことによる。

### 割り当ての出どころ

`strace -f -c` では、musl の版だけ、`--jobs 8` で futex が約 2 万回あった（glibc は 39 回）。
`perf record -e syscalls:sys_enter_futex -g` で futex の呼び出し元をたどると、大半は CFF のグリフの charstring を初めて解釈するところだった。

- charstring の引数のスタックは `Vec<i64>` で、演算子が引数を `mem::take` で取るので、次の数を積むたびに新しく割り当てる（[src/font/cff.rs:246](https://github.com/tett23/shem/blob/0e22817d6437babaa4aadca3dc3bd959fba6759d/src/font/cff.rs#L246)、[src/font/cff.rs:320-322](https://github.com/tett23/shem/blob/0e22817d6437babaa4aadca3dc3bd959fba6759d/src/font/cff.rs#L320-L322)）。
- 解釈の結果の `Vec<Segment>` は、伸びるたびに realloc する。
- 一つのグリフの解釈は文書ごとに一度だけ（[ADR 0011](0011-speed-up-without-changing-bytes.md)）だが、kanikosen のページには初めて出る漢字が多い。107 種類のグリフを初めて検査すると、6,395 回の割り当てがあった。
- [ADR 0013](0013-fewer-allocations-for-musl-threads.md) の割り当ての回数の検査は、アウトラインをキャッシュに入れてから数えていたので、これを見逃していた。

ほかに、ページごとに次のものがあった。

- `Tj` の 16 進の文字列を `Vec<u8>` に解く（[src/pdf/lexer.rs:191](https://github.com/tett23/shem/blob/0e22817d6437babaa4aadca3dc3bd959fba6759d/src/pdf/lexer.rs#L191)）。グリフ 1 個につき、検査と描画で 2 回である。
- ページごとに作る作業用の `Vec`（演算子、セル、パス、画像）が伸び、ページの終わりに解放する。mallocng は、空になったグループや大きな割り当てを、錠を持ったまま munmap で返す（[malloc.c](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/malloc.c)、[free.c](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/free.c)）。

### CPU の時間の出どころ

割り当てとは別に、`perf record` では、次のものが CPU の時間の多くを占めていた。どれも、スレッドの数によらず時間を延ばす。

- 十進の数の読み（15%）：小数部を、2 倍しては 1 桁ずつ繰り上げて 33 回たどる（[src/fixed.rs:148](https://github.com/tett23/shem/blob/0e22817d6437babaa4aadca3dc3bd959fba6759d/src/fixed.rs#L148)）。Golem は `Tm` に小数を書くので、グリフごとに三つ読む。
- 縦書きの寸法の検索（7%）：W2 の範囲を先頭から順にたどる（[src/font.rs:98](https://github.com/tett23/shem/blob/0e22817d6437babaa4aadca3dc3bd959fba6759d/src/font.rs#L98)）。
- 検査の段階（全体の約 29%）：画素を描かないのに、グリフのパスを作り、曲線を平坦化し、線の多角形を作る。
- 塗りのセルの並べ替え（約 13%）と、`render_line` のイテレーターの連鎖（[src/raster.rs:187](https://github.com/tett23/shem/blob/0e22817d6437babaa4aadca3dc3bd959fba6759d/src/raster.rs#L187)、[src/raster.rs:228](https://github.com/tett23/shem/blob/0e22817d6437babaa4aadca3dc3bd959fba6759d/src/raster.rs#L228)）。

### 試作での確かめ

上の出どころを順に直す試作を作り、同じ 8 CPU の VM で測った（3 回の中央値、ミリ秒）。段階ごとに別の回に測ったので、同じビルドでも回によって 5% ほど揺れる。

| 段階 | musl `--jobs 1` | musl `--jobs 8` |
|---|---|---|
| v0.1.0-rc3 | 1069 | 260 |
| 16 進の文字列を借りる | 1066 | 222 |
| スレッドごとの作業用の場所 | 1045 | 223 |
| charstring のスタックを固定の配列にする | 1048 | 200 |
| charstring の結果をスレッドのバッファに置く | 1047 | 194 |
| 十進の数と W2 の検索 | 770 | 161 |
| 検査でパスを作らない、小さなパスを密な格子で塗る | 756 | 158 |
| `render_line` を素直なループにする | 580 | 131 |

- どの段階でも、出力は v0.1.0-rc3 とバイト単位で同じだった。`tests/kanikosen.rs` と `tests/golden.rs` も、作り直さずに通った。
- スレッドごとの作業用の場所（アリーナの案）は、単独では `--jobs 8` を変えなかった。最後の段階で、ページごとに作業用の場所を作り直す版と比べると、`--jobs 8` が 195 ミリ秒から 186 ミリ秒、`--jobs 1` が 1059 ミリ秒から 1031 ミリ秒（5 回の中央値）で、5% ほど速かった。
- 検査でパスを作らない変更と密な格子は、それだけでは時間を変えなかった。並べ替えが消えたかわりに、`render_line` が重くなったためである。`render_line` を書き直して、はじめて効いた。

## 実装すること

### 原則

- どの変更も、画素と PNG と PPM のバイト列を変えない。報告するエラーも変えない。[ADR 0011](0011-speed-up-without-changing-bytes.md)、[ADR 0013](0013-fewer-allocations-for-musl-threads.md) と同じく、ゴールデンと kanikosen のバイト列の一覧を作り直さずに通すことを条件とする。
- 依存は増やさない。配布物のターゲット（musl）も変えない。`unsafe` も使わない。

### 割り当て

- **16 進の文字列**：字句は、16 進の文字列を、検査したうえで `<` と `>` の間のスライスとして返す（`Token::HexString`）。オブジェクトにするときに解く。`Tj` の文字列は、16 進ならスライスのまま持ち、グリフを描くときに解きながら読む（`content::Text`）。`Text` の等しさと `Debug` の表示は、解いたバイト列で決める。
- **charstring のスタック**：引数のスタックを、長さ 48（Type 2 の上限）の固定の配列にする。演算子は配列を写して受け取る。
- **charstring の結果**：グリフのアウトラインを、呼び出し元のバッファに解釈し、キャッシュへは `Arc<[Segment]>` として一度だけ写す（`Font::outline_with`）。描画は、このバッファをスレッドの作業用の場所に持つ。
- **スレッドごとの作業用の場所**：`render::Workspace` に、演算子の `Vec`、グラフィックス状態のスタック、三つのパス、塗りの作業用の場所、アウトラインのバッファ、画像を持たせる。スレッドは一つの `Workspace` を、検査と描画のすべてのページで使い回す。演算子の `Vec` は内容のバイト列を借りるので、ページの間では空にして、標準ライブラリがその場で集める（in-place collect）ことで割り当てを引き継ぐ。引き継がれなくても、結果は同じである。
- **並列の実行**：`parallel::run_with(count, jobs, init, task)` を加える。スレッドが始まるときに `init` で状態を作り、`task(&mut state, i)` に渡す。`parallel::run` は、状態のない `run_with` とする。添字の配り方、失敗の扱い、返すエラーは [ADR 0009](0009-render-pages-in-parallel.md) のまま変えない。

### ライブラリの API

- `Session::check_with(&mut Workspace, page)` と `Session::render_with(&mut Workspace, page) -> Result<&Canvas>` を加える。画像は、次のページまで `Workspace` の中に残る。
- `Session::render_pages` の `consume` は、画像を値ではなく参照（`Fn(usize, &Canvas)`）で受け取る。画像の領域を次のページで使い回すためである。[ADR 0012](0012-parallel-library-api.md) の「v0.x の間は版を上げずに変えうる」に当たる。`check`、`render`、`render_page` などの形は変えない。

### CPU の時間

- **十進の数**：小数部が 19 桁以下なら、D / 10^n の D · 2^32 を一度割り、余りで最近接の偶数に丸める（10^19 · 2^32 は u128 に収まる）。20 桁以上は、今の 2 倍の繰り返しで読む。
- **W と W2**：範囲が昇順で重ならないときは、二分探索で引く。そうでないときは、今と同じく先頭からたどる。どちらも、CID を含む最初の範囲を返す。
- **検査**：画素を描かないときは、グリフの点をデバイス空間に写すだけにして（桁あふれのエラーはここで出る）、パスと線の多角形は作らない。パスを作る操作は失敗しない。
- **密な格子**：塗るパスの点の外接矩形から、画素を変えうるセルの行と列（行は画像の中、列は −1 から画像の右端まで）を求める。その数が 2^16 以下なら、セルを並べ替えずに密な格子へ足し込む。画像の左のセルは列 −1 にまとめ、右と上下のセルは捨てる。並べ替える場合と同じ画素になる（左のセルは行の覆いに足されるだけで、右と上下のセルはどの画素も変えず、値が 0 のセルはそれを含む区間の画素と同じ画素を出す）。超えるときは、今と同じく並べ替える。
- **`render_line`**：行と列の交点を、`once` と `chain` をつないだイテレーターではなく、素直なループでたどる。交点の計算の式と、セルを足す順は変えない。

### 測定と文書

- 変更の前と後を、手元の 8 CPU の VM と、ワークフロー Benchmark（[ADR 0013](0013-fewer-allocations-for-musl-threads.md)、ubuntu-24.04、4 vCPU）で測り、`docs/benchmarks/` に記録として残す。
- 仕様（ライブラリの節と決定性の節）を、この ADR に合わせて書き直す。
- 出力を変えないので、リリースはパッチバージョンとする（[ADR 0001](0001-scope-golem-contract-and-determinism.md)）。リリースノートに、`render_pages` の `consume` の形の変更を書く。

## 実装しないこと

- 汎用のアリーナ（ページの間で中身ごと捨てる割り当て器）と、自前の全体のメモリ割り当て。必要な `Vec` を名前つきで持つ `Workspace` で足り、割り当て器を作ると `unsafe` が要る。
- 座標の変換（`to_device`）を検査で省くこと。今は、検査と描画の CPU の時間の約 27% を占める。省くには、グリフの外接矩形から桁あふれしないことを示す仕組みが要るので、別の ADR で検討する。
- PNG の書き出しの使い回しと高速化。Golem の CI は PPM で描く。PNG の大きな割り当ては 1 ページあたり約 7 回で、ページの数に比例するだけである。
- TrueType のグリフの解釈の割り当て（グリフごとに約 7 回で、文書ごとに一度だけ）。Golem の TrueType のフォントは欧文だけで、グリフの種類が少ない。
- 既定の `--jobs` の変更。

## テスト設計

### 出力が変わらないこと

- 既存のゴールデン（`tests/golden.rs`）、kanikosen の 516 個のファイルのバイト列（`tests/kanikosen.rs`）、fixture のすべてのページの演算子の列（`tests/golem_fixtures.rs`）を、作り直さずに通す。演算子の列の一覧は `Debug` の表示の SHA-256 なので、`Text` の `Debug` が解いたバイト列を表示することもここで確かめる。

### 変わった関数

- 十進の数：19 桁以下のランダムな小数（固定の種、2 万個。同じ近さの値、5 の並び、0 と 9 の並びを含む）で、新しい読み方が 2 倍の繰り返しと同じ値になる。20 桁以上の値も今と同じになる。
- W と W2：ランダムな範囲の並び（固定の種、300 通り。三つに一つは重なりや逆行を含む）の 0〜319 のすべての CID で、先頭からたどる検索と同じ値を返す。
- 密な格子：[ADR 0013](0013-fewer-allocations-for-musl-threads.md) の参照の実装と、画像の大きさとパスの広がりを変えて同じになる（12 × 9 の画像に小さなパス、600 × 500 の画像に格子の上限を超えるパス、画像の左上と右下の外にあるパス）。同じ `Scratch` を続けて使う。
- `render_line`：上の比較と、[ADR 0013](0013-fewer-allocations-for-musl-threads.md) の参照との比較で確かめる。
- `parallel::run_with`：`init` はスレッドの数以下の回数だけ呼ばれ、タスクは自分のスレッドの状態を受け取り、すべての添字が一度ずつ実行される（件数 0、1、100、`jobs` 1、3、4、16）。
- `Workspace`：fixture のページを、逆順、正順、飛ばしながら、同じ `Workspace` で検査して描いた画像が、`Session::render` と同じになる。途中に存在しないページのエラーを挟んでも、大きさと解像度の違う文書のページを続けても同じになる。

### 割り当ての回数

- `tests/allocations.rs` に、初めて解釈するグリフの検査を加える。kanikosen の 3 ページ目に出る 107 種類のグリフを 1 個ずつ並べたページを、フォントだけを読んだキャッシュで検査し、二度目の検査との割り当ての差が、グリフあたり 4 回以下である。変更の前は 6,395 回（グリフあたり約 60 回）で、このテストは失敗する。

### 速さ

- 自動のテストにはしない。上の「測定と文書」のとおり記録する。

## トレードオフ

- `Workspace` と `run_with` で、スレッドの状態を明示的に渡す形にしたので、隠れた状態（スレッドローカルの変数）はない。そのかわり、描画の関数に引数が一つ増え、ライブラリの `consume` は画像を参照で受け取る形に変わる。画像を手元に残したい使い手は、`clone` する必要がある。
- `Workspace` の効き目は 5% ほどと小さい。大きく効いたのは、charstring と数の読みと `render_line` である。それでも、ページの間の munmap と、スレッドが錠を取る回数を減らすので残す。
- 演算子の `Vec` の使い回しは、標準ライブラリのその場で集める最適化に頼る。最適化が働かなくても結果は同じで、割り当てが一つ増えるだけである。
- 密な格子は、外接矩形が大きいパス（ページ全体の長方形など）では使わない。上限を 2^16 セル（1 MiB）にしたので、作業用の場所は 1 スレッドあたり最大 1 MiB 増える。
- 速くなった分の多くは、スレッドの数によらない。musl と glibc の差は、8 スレッドで 5% ほど残る。

## 参考文献

- 2026-11-05 の依頼「--jobs を増やしたとき、動作が遅くなる。アロケーションが原因なので単なるアイデアですが、内部でアリーナを持つことも考慮して高速化してください」
- musl の mallocng（GitHub の写し kraj/musl の bd54dc3e）：[malloc.c](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/malloc.c)、[free.c](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/free.c)、[glue.h](https://github.com/kraj/musl/blob/bd54dc3e8049b9c7db092905e743cdebbb8864f3/src/malloc/mallocng/glue.h)
- glibc の malloc の調整の変数（`MALLOC_MMAP_THRESHOLD_`、`MALLOC_ARENA_MAX`） https://www.gnu.org/software/libc/manual/html_node/Memory-Allocation-Tunables.html
- Adobe Technical Note #5177「The Type 2 Charstring Format」（引数のスタックの上限 48）
- Rust の `Vec` の `FromIterator` の割り当ての振る舞い（in-place iteration） https://doc.rust-lang.org/std/vec/struct.Vec.html#impl-FromIterator%3CT%3E-for-Vec%3CT%3E
