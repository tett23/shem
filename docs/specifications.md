# shem 仕様書

shem は、PDF のページを PNG の画像に描くラスタライザである。
最初の目的は、Golem（https://github.com/tett23/golem ）の PDF の出力の見た目の回帰テストで、見た目の変化を見つけることである。
描いた結果が正しいことを示すものではない。
v1 では、汎用の PDF のラスタライザを目指す（[roadmap.md](roadmap.md)）。

この文書は、shem の今の振る舞いを定める。
設計の決定は `docs/adr/` に連番の ADR として置く。

## 状態

v0.1 の範囲（[ADR 0001](adr/0001-scope-golem-contract-and-determinism.md)〜[0012](adr/0012-parallel-library-api.md)）、musl の配布物でスレッドを増やすと遅くなる問題の修正（[ADR 0013](adr/0013-fewer-allocations-for-musl-threads.md)）、スレッドごとの作業用の場所と、検査と塗りの高速化（[ADR 0014](adr/0014-thread-workspaces-and-faster-check-and-fill.md)）、PNG の書き出し、描画、Flate の展開の高速化（[ADR 0015](adr/0015-faster-png-render-and-decoding.md)）、ブラウザと Deno で動かす wasm のモジュール（[ADR 0018](adr/0018-wasm-module-for-the-browser.md)）、Deflate の探索、密な格子の掃き出し、演算子の数の高速化（[ADR 0019](adr/0019-faster-deflate-sweep-and-operands.md)）、交点の持ち越しを i64 のどの座標でも正しくする変更（[ADR 0020](adr/0020-carried-crossings-for-every-i64.md)）、悪意のある入力への上限（[ADR 0021](adr/0021-limits-on-hostile-input.md)）、キャンバスの外の線分と曲線に時間をかけない変更（[ADR 0022](adr/0022-no-work-for-geometry-off-the-canvas.md)）、線の継ぎ目の miter の点の求め方（[ADR 0023](adr/0023-miter-points-within-the-limit.md)）、リリースの出自の証明と、CI とリリースの固定と保護（[ADR 0024](adr/0024-provenance-and-pinning-for-ci-and-releases.md)）、ページのパスの線と、丸い線端と破線（[ADR 0025](adr/0025-strokes-of-paths-with-round-caps-and-dashes.md)、[ADR 0026](adr/0026-corrections-to-round-caps-and-dashes.md)）を実装し、v0.1.0-rc8 をプレリリースとして出した。
版ごとの計画は [roadmap.md](roadmap.md) にある。

## コマンド

```
shem render [--dpi <N>] [--format png|ppm] [--pages <範囲>] [--jobs <N>] <入力の PDF> <出力のディレクトリ>
shem --version
shem --help
```

`render` は、入力の PDF のページを一ページ一ファイルで描く（[ADR 0007](adr/0007-png-output-resolution-and-cli.md)）。

- `--dpi`：1 以上 1200 以下の整数。既定は 150。
- `--format`：`png`（既定）か `ppm`。
- `--pages`：`3`、`2-5`、`1,3,5-7` の形で、1 から数える。既定はすべてのページ。重なりは一つにまとめる。形が誤っていれば使い方の誤り、ページの数を超えればエラーとする。
- `--jobs`：同時に描くページの数の上限で、1 以上 1024 以下の整数。既定は使えるコアの数（`std::thread::available_parallelism`、取れなければ 1）。出力のバイト列と報告するエラーは `--jobs` によらない（[ADR 0009](adr/0009-render-pages-in-parallel.md)）。
- オプションは `render` の後ろならどこに置いてもよい。同じオプションを二度書くのは使い方の誤りとする。
- 出力のディレクトリがなければ親も含めて作る。ファイルの名前は `page-<番号>.<拡張子>` で、番号はページの数の桁数と 4 のうち大きいほうの桁数に 0 で詰める。同じ名前のファイルは置き換え、ほかのファイルには触れない。ファイルは `.page-0001.png.tmp` のような一時的な名前で書いてから名前を変える。
- 書く前に、描くすべてのページを、画素を描かずに解釈して検査する（[ADR 0002](adr/0002-pdf-subset-and-out-of-scope-input.md)）。範囲の外の入力があれば、ファイルを一つも書かずに終える。
- 検査と描画は、ページを単位に並列に行う。一つのページは一つのスレッドで描く。ページの番号は小さい順に配り、エラーが出たら新しいページを配らず、実行中のページは最後まで処理する。報告するのは、エラーになったページのうち番号の最も小さいものとする。描画か書き出しのエラーでは、それまでに書いたファイルは残る（[ADR 0009](adr/0009-render-pages-in-parallel.md)）。
- 標準出力には何も書かない。エラーは標準エラーに `shem: <入力のパス>: <場所>: <内容>` の一行で書く。
- 終了コードは、成功が 0、入力が範囲の外か壊れているか出力に失敗したときが 1、使い方の誤りが 64 とする。

### PNG と PPM

- PNG は `IHDR`（8 ビット、RGB、インターレースなし）、一つの `IDAT`、`IEND` だけを書く。行のフィルタは、五つの型のうち、フィルタ後のバイトを符号つきとみなした絶対値の和が最も小さいもの（同じなら番号の小さいもの）を選ぶ。
- `IDAT` は zlib の形式で、ヘッダーは `78 01`。Deflate は、32 KiB のウィンドウと 3 バイトのハッシュの鎖（たどるのは 128 まで）で最も長い一致（同じ長さなら近いもの）を選ぶ貪欲法と、固定のハフマンの符号の一つのブロックで書く。CRC-32 と Adler-32 は自前で計算する。
- PNG のバイト列は、同じ版の shem で同じ画素なら同じになる。版が違えば、画素が同じでも変わることがある。
- PPM は `P6\n<幅> <高さ>\n255\n` の後に画素を並べたもので、バイト列は画素だけで決まる。

## 差分の画像

`shem diff [--format png|ppm] [--jobs <N>] <前> <後> <出力>` は、shem が書いた画像を比べ、違う画素に色を付けた差分の画像を書く（[ADR 0010](adr/0010-diff-images.md)）。

- `<前>` と `<後>` がどちらもファイルなら、二枚を比べる。違えば `<出力>` に差分の画像を書き、標準出力に `differ: <違う画素の数> pixels in <x0> <y0> <x1> <y1>` を一行書く（`x1` と `y1` は含まない）。同じなら何も書かない。`<出力>` のディレクトリは作らない。
- どちらもディレクトリなら、直下の `.png` と `.ppm` のファイル（`.` で始まるものを除く）を同じ名前どうしで比べる。違う組だけ、`<出力>` のディレクトリに `<拡張子を除いた名前>.<png|ppm>` で差分の画像を書く。標準出力には、名前の順に `differ: <名前>: ...`、`added: <名前>`（後にだけある）、`removed: <名前>`（前にだけある）を一行ずつ書く。出力のディレクトリがなければ作り、ほかのファイルには触れない。
- 片方がファイルでもう片方がディレクトリなら、使い方の誤りとする。
- 組は `--jobs`（既定は使えるコアの数）で並列に比べる。標準出力の行と報告するエラー（名前の順で最初のもの）は `--jobs` によらない。
- 読むのは、8 ビットの RGB でインターレースのない PNG（CRC を確かめ、補助のチャンクを読み飛ばす）と、最大値 255 の P6 の PPM だけで、ほかはエラーにする。画素の数が 2^28 を超える画像もエラーにする。PNG の `IDAT` は、`IHDR` から決まる大きさ `高さ × (幅 × 3 + 1)` を超えて伸長すれば、そのバイトを書く前に、大きさの不一致と同じ文 `malformed image: the image data does not have the size of the image` のエラーにする（[ADR 0021](adr/0021-limits-on-hostile-input.md)）。
- 差分の画像の大きさは、二枚の幅と高さのそれぞれ大きいほうとする。その画素の数が 2^28 を超えれば、画素を作る前に `the diff image is too large: <幅> × <高さ> pixels` のエラーにし、`<後>` の画像のエラーとして報告する（[ADR 0021](adr/0021-limits-on-hostile-input.md)）。明るさを `L = (299 R + 587 G + 114 B + 500) / 1000` とし、片方にしかない画素を緑 `(0, 160, 0)`、後のほうが暗い画素を赤 `(230, 0, 0)`、明るい画素を青 `(0, 90, 255)`、明るさが同じで色が違う画素を紫 `(180, 0, 180)`、同じ画素を `255 − (255 − L) / 4` の灰色にする。違う画素を囲む矩形の 2 画素から 3 画素外に、橙 `(255, 160, 0)` の枠を、同じ画素の上にだけ描く。
- 終了コードは、違いがなければ 0、違いがあれば 1、画像を読めないか出力に失敗したら 2（標準エラーに `shem: <パス>: <内容>`）、使い方の誤りなら 64 とする。

## ライブラリ

`shem` のクレートは、コマンドが使う描画の API を公開する（[ADR 0012](adr/0012-parallel-library-api.md)）。v0.x の間、この API は版を上げずに変えうる。

- `shem::parallel::run(count, jobs, task)` は、`task(0)` から `task(count − 1)` を最大 `jobs` 個のスレッドで実行する。番号は小さい順に一度ずつ配り、エラーの後は新しい番号を配らず、実行中の仕事は最後まで実行する。返すのは、エラーになった番号のうち最も小さいものとそのエラーで、スレッドの数と終わる順によらない。`jobs` が 1 なら、呼び出したスレッドで順に実行する。仕事のパニックは呼び出し元に伝わる。`shem::parallel::available_jobs()` は使えるコアの数（取れなければ 1）を返す。
- `shem::parallel::run_with(count, jobs, init, task)` は、`run` と同じ順と規則で、スレッドごとの状態を渡す。スレッドが始まるときに `init()` で状態を作り、`task(&mut state, i)` に渡す。`run` は、状態のない `run_with` である（[ADR 0014](adr/0014-thread-workspaces-and-faster-check-and-fill.md)）。
- `shem::render::Session` は、一つの文書を一つの解像度で描く入り口で、フォントのキャッシュ（[ADR 0011](adr/0011-speed-up-without-changing-bytes.md)）と復号した画像のキャッシュ（[ADR 0027](adr/0027-images-and-figures.md)）をページ、検査と描画、スレッドの間で共有する。ページは 0 から数える。`check` と `render` は一つのページを、`check_pages` と `render_pages` は複数のページを並列に扱い、`pages` の並びで最も前のページのエラーを返す。`render_pages` は、描いた画像の参照（`&Canvas`）を、描いたスレッドで `consume` に一度ずつ渡す。画像の領域は、そのスレッドの次のページで使い回すので、残したい画像は `consume` の中で写す（[ADR 0014](adr/0014-thread-workspaces-and-faster-check-and-fill.md)）。`consume` のエラーは `PageError::Consume`、描画のエラーは `PageError::Render` になる。
- `shem::render::Workspace` は、スレッドが検査と描画のページの間で使い回すバッファ（演算子、グラフィックス状態のスタック、パス、塗りの作業用の場所、アウトラインのバッファ、画像）である。`Session::check_with(&mut ws, page)` と `Session::render_with(&mut ws, page)` は、その中で一つのページを扱い、`render_with` の画像は次のページまで `ws` の中に残る。同じ `Workspace` で続けて扱っても、結果は `check` と `render` と同じである。`check_pages` と `render_pages` は、スレッドごとに一つの `Workspace` を使う。
- `Session::with_jobs(doc, dpi, jobs)` は、ページの内容の Flate の展開を最大 `jobs` 個のスレッドで行う `Session::new` である。ページの木は一つのスレッドでたどり、エラーは番号の最も小さいページのもので、一つのスレッドと同じである。`shem::pdf::Document::pages_with(jobs)` も同じで、`shem render` は `--jobs` の値で使う（[ADR 0015](adr/0015-faster-png-render-and-decoding.md)）。
- `Session::from_pages(doc, pages, dpi)` は、先に読んだページ（`Document::pages_with`）を借りる `Session` である。検査が通ったページの演算子を持ち、そのページの描画は内容を解析しなおさずにそれを使う。検査していないページは描画で解析する。結果は `Session::new` と同じである。`shem render` はこれを使う（[ADR 0015](adr/0015-faster-png-render-and-decoding.md)）。
- `shem::raster::Canvas::fill` は、パスの覆いを `Vec` に集めずに、掃きながら画像に重ねる。結果は、`coverage` を `Canvas::paint` したものと同じである（[ADR 0015](adr/0015-faster-png-render-and-decoding.md)）。
- `shem::error::Error` は、場所（ページ、オブジェクト、位置）と文を `Box` の中に持ち、`page()`、`object()`、`offset()`、`message()` で読む（[ADR 0019](adr/0019-faster-deflate-sweep-and-operands.md)）。
- `render_page` と `check_page` は、一つのページのための `Session` で描く。
- `Session` はスレッドの間で共有でき（`Sync`）、画像（`Canvas`）はスレッドの間で受け渡せる（`Send`）。

## wasm のモジュール

ブラウザ（Web Worker）と Deno で動かすために、ライブラリを `wasm32-unknown-unknown` で `shem.wasm` にビルドする（[ADR 0018](adr/0018-wasm-module-for-the-browser.md)）。

- import はない。`WebAssembly.instantiate(module, {})` で読み込める。エクスポートは下の関数と `memory` だけである。ビルドには `rust-toolchain.toml` の版の stable の Rust を使う（古い nightly のリンカーは、ほかに二つのグローバルをエクスポートする）。
- 関数は C の ABI で、引数と戻り値は 32 ビットの整数（線形メモリの中のポインタ、長さ、番号）である。

| 関数 | 引数 | 戻り値 | 意味 |
|---|---|---|---|
| `shem_alloc` | `len` | ポインタ、確保できなければ 0 | `len` バイトの入力の場所を確保して、0 で埋める |
| `shem_free` | `ptr` | なし | `shem_alloc` の場所を解放する |
| `shem_open` | `ptr`、`len`、`dpi` | 文書の番号（1 以上）、失敗なら 0 | 入力の最初の `len` バイトを PDF として読み、ページの木と内容を読む |
| `shem_page_count` | `doc` | ページの数、文書がなければ 0 | |
| `shem_render` | `doc`、`page` | 成功なら 1、失敗なら 0 | 0 から数えたページを描き、文書の画像にする |
| `shem_image_ptr`、`shem_image_width`、`shem_image_height` | `doc` | 最後に描いた画像の RGBA の先頭のポインタ、幅、高さ | 文書がなければ 0 |
| `shem_close` | `doc` | なし | 文書と画像を解放する |
| `shem_error_ptr`、`shem_error_len` | なし | 最後のエラーの文の先頭のポインタと、バイト数 | UTF-8 |

- 入力は、`shem_alloc` の返したポインタで探す。確保していないポインタか、確保した長さを超える長さなら、`shem_open` は失敗する。`shem_open` は入力を写して持つので、開いた後に `shem_free` してよい。
- `shem_alloc` は、`len` バイトを確保できなければトラップせずに 0 を返し、エラーの文を `cannot allocate <len> bytes` にする。確保した場所のポインタは 0 にならない（[ADR 0021](adr/0021-limits-on-hostile-input.md)）。
- 画像の RGBA は、描いた RGB に不透明度 255 を足したもので、行は上から、画素は左からの順に並ぶ。画像は、同じ文書の次の `shem_render` か `shem_close` まで有効である。
- `shem_render` は、描くページだけを検査して描く。`shem render` のように、すべてのページを先に検査することはしない。フォントは、文書ごとにページの間で使い回す。
- 失敗した関数は 0 を返し、エラーの文は次の失敗まで読める。文は、`shem render` が標準エラーに書く文の `shem: <パス>: ` の後と同じである。番号の違う文書（`no such document: N`）、範囲の外のページ（`page N does not exist: the document has M pages`）、1 以上 1200 以下でない DPI（`dpi must be an integer from 1 to 1200: N`）、確保していない入力（`no such input: P`）、長すぎる長さ（`length L is more than the input of N bytes`）も同じく失敗にする。
- パニックはトラップ（`WebAssembly.RuntimeError`）になり、エラーとしては返せない。トラップしたインスタンスは捨てて、読み込みなおす。壊れた PDF 5,000 個でパニックしないことを、ネイティブのテストで確かめる。
- 描く画素は、ネイティブの shem と同じである。CI の `wasm` のジョブは、Deno で `shem.wasm` を読み込み、kanikosen を 144 DPI で描いた 129 ページ（どれも 726 × 1032）の RGB が、ネイティブの PPM の画素とバイト単位で一致することを確かめる。

## 読む PDF の部分集合

v0.1 は、今の Golem の Writer が書く PDF の部分集合を読む（[ADR 0002](adr/0002-pdf-subset-and-out-of-scope-input.md)）。
範囲の外の入力と壊れた入力は、エラーで止める。読み飛ばしも、近いものでの代用もしない。
エラーは `page <番号>`、`object <番号>`、`offset <バイトの位置>` のうちわかるものと内容を、`page 3: offset 120: unsupported operator TJ` の形で示す。

### ファイルの構造

- 先頭は `%PDF-` で始まる。末尾の 1024 バイトにある `startxref` から、古典的な xref の表と trailer を読む。
- xref のストリーム、オブジェクトのストリーム（`/Type /ObjStm`）、trailer の `Prev`、`XRefStm`、`Encrypt`、世代番号が 0 でないオブジェクトは、エラーにする。xref の表の位置にそのオブジェクトがないときもエラーにする。
- オブジェクトは使うときに読む。ストリームの `Length` は直接の整数か、整数への間接参照とする。
- 中身を読むストリームのフィルタは、なしか `FlateDecode` だけとする。`DecodeParms` はエラーにする。Flate の伸長は自前で行い、Adler-32 が合わなければエラーにする。
- ページの木をたどり、`Resources`、`MediaBox`、`Rotate` を継承する。木の循環はエラーにする。`Rotate` は 0 だけを認め、`UserUnit` はエラーにする。`Contents` の配列は、ストリームの間に空白を一つ挟んでつなぐ。注釈は読まない。

### ページの内容の演算子

解釈する演算子は `q Q cm w J j M d m l c h re f f* S B B* n W W* k K cs CS sc SC scn BT ET Tf Tz Tm Tr Tj Do BDC EMC` だけで、ほかの演算子は `unsupported operator <演算子>` のエラーにする。
`J`、`d`、`l`、`S` は、Golem が角丸の囲みの枠を線で描くために読む（[ADR 0025](adr/0025-strokes-of-paths-with-round-caps-and-dashes.md)）。
`j`、`M`、`2 J`、`f*`、`B`、`B*`、`n`、`W`、`W*`、`CS`、`sc`、`SC` は、Golem の図版の Form が書くために読む（[ADR 0027](adr/0027-images-and-figures.md)）。

- 被演算子の数と型が合わなければ `wrong operands of <演算子>` のエラーにする。`w` の負の値もエラーにする。`Tr` は 0 と 2 だけを認める。
- `J` は整数 1 で、0（butt）、1（round）、2（square）を認め、ほかの整数は `malformed J <値>` のエラーにする。
- `j` は整数 1 で、0（miter）、1（round）、2（bevel）を認め、ほかの整数は `malformed j <値>` のエラーにする。`M` は数 1 で、1 未満は `malformed M` のエラーにする。初期値は `0 j` と `10 M` である。
- `d` は配列 1 と数 1 で、配列の要素は 0 以上の数、位相は 0 以上の数とする。負の数と数でない要素は `wrong operands of d`、空でない配列の要素がすべて 0 なら `malformed d` のエラーにする。空の配列は実線を表す。
- `sc` と `SC` は 1 個から 4 個の数を取る。`CS` は名前 1 を取る。
- `q` と `Q`、`BT` と `ET`、`BDC` と `EMC` は対応する。文字のオブジェクトの中では `q Q cm Do BT` とパスの演算子を、パスの途中では `m l c h re W W*` とパスを終える演算子 `f f* S B B* n` 以外を、エラーにする（ISO 32000-1 の附属書 L の図 9）。`l`、`c`、`h`、`W`、`W*` は現在の点を要る。`J`、`j`、`M`、`d`、`CS`、`sc`、`SC` は `w` と同じ場所に置ける。
- `W` と `W*` の直後には、パスを終える演算子だけを置ける。ほかの演算子は `<演算子> after W` のエラーにする。
- `Tf`、`cs`、`CS`、`Do` の名前は、ページの `Resources` の `Font`、`ColorSpace`、`XObject` になければエラーにする。

### 入れ子、フォントの解釈、伸長の上限

小さな入力でスタック、時間、メモリを使い尽くす入力は、次の上限を超えたところで止める（[ADR 0021](adr/0021-limits-on-hostile-input.md)）。
上限を超える入力は範囲の外として扱い、描く前の検査でエラーにする。

- 配列と辞書の入れ子は 64 段までとする。trailer、オブジェクトの本体、内容のストリームの被演算子のどれも同じで、65 段目を開く `[` か `<<` の位置で `arrays and dictionaries nested deeper than 64` のエラーにする。
- ページの木は、根の `Pages` を 1 段目として 64 段までとする。65 段目のノードのオブジェクトの番号で `page tree nested deeper than 64` のエラーにする。循環の検査はそのまま行う。
- CFF の一つのグリフの解釈は、本体とサブルーチンの数と演算子を一つずつ数えて 65,536 段までとし、超えれば `charstring longer than 65536 steps` のエラーにする。呼び出しの深さ（10）と引数のスタック（48）の上限は変わらない。
- TrueType の一つのアウトラインが読むグリフは、自身と部品を合わせて 256 個までとし、超えれば `composite glyph of more than 256 glyphs` のエラーにする。入れ子の深さ（8）の上限は変わらない。
- 一つのストリームの伸長の後の大きさは 2^26 バイト（64 MiB）までとする。上限を超えるバイトを書く前に `decoded data larger than 67108864 bytes` のエラーにするので、伸長に確保するメモリは上限の 2 倍を超えない。`Contents` の配列は、ストリームごとに上限を当てる。文書全体の上限は置かない。
- Form の入れ子は、ページを 0 段目として 8 段までとし、9 段目の Form で `forms nested deeper than 8` のエラーにする（[ADR 0027](adr/0027-images-and-figures.md)）。一つの画像の標本は `Width × Height × 成分の数` が 2^26 までとし、超えれば標本を読む前に `image larger than 67108864 samples` のエラーにする。画像の標本の伸長は、標本の大きさで打ち切る。

## 描画

パスを画素にするまでの計算は、整数と固定小数点だけで行う（[ADR 0003](adr/0003-paths-fill-stroke-and-antialiasing.md)）。

- 装置の空間は左上を原点とし、x を右、y を下に向ける。ページの初期の CTM は `[s 0 0 −s −s·llx s·ury]`（`s = dpi / 72`、`MediaBox` が `[llx lly urx ury]`）で、`cm` は `CTM ← M × CTM` とする。
- 画像の幅と高さは、`MediaBox` の幅と高さに `dpi / 72` を掛けて切り上げる。0 になるか、画素の数が 2^28 を超えればエラーにする。
- パスの点は、作るときに CTM で装置の空間に写し、画素の 1/256 の整数に丸める。`l` は現在の点から線分を加え、`re` は `m`、三つの `l`、`h` と同じとする。
- 3 次のベジエ曲線は、制御点の二階差分の大きいほうが 1/16 画素以下になるまで中点で二分し、線分の列にする。深さは 16 までとする。
- 塗りは `f` と `B` で nonzero の規則、`f*` と `B*` で偶奇の規則とする（[ADR 0027](adr/0027-images-and-figures.md)）。覆いは ftgrays と同じく、セルごとの cover と area を整数で積み、行ごとに左から累積して求め、`(v × 255 + ONE/2) / ONE` で 0 から 255 の不透明度にする。
- ページは白で始まる。塗る色 `c` は、不透明度 `a` で画素 `p` に `(c × a + p × (255 − a) + 127) / 255` として合成する。塗りの演算子ごとに、パス全体の覆いを求めてから一度だけ合成する。
- `B` と `B*` は、パスを塗ってから線として描く。`n` は何も描かずにパスを空にする（[ADR 0027](adr/0027-images-and-figures.md)）。
- 線は、区間ごとの四角形と継ぎ目の多角形を、向きを正にそろえて nonzero で塗る。線端は `J`（0 butt、1 round、2 square）、角は `j`（0 miter、1 round、2 bevel）で選ぶ（[ADR 0027](adr/0027-images-and-figures.md)）。miter の点は、二つの区間の方向から `half · (N1 + N2) / (1 + cos θ)`（`N1` と `N2` は単位の法線、`θ` は方向の間の角）として求め、方向の成分を 2^28 未満にシフトし、長さを 2^−32 の精度にした整数の計算で丸める。頂点からの距離は、線の幅の半分の 16 倍以内である（[ADR 0023](adr/0023-miter-points-within-the-limit.md)）。線の幅は利用者の空間の値に `sqrt(|det CTM|)` を掛ける。幅 0 は 1 画素とする。
- miter の上限は `M` とする。二つの区間の方向の内積を `dot`、長さの積を `lens` として、`M² · (lens + dot) ≥ 2 · lens` なら miter、そうでなければ bevel とする。`M` の 2 乗は Q32.32 の値の 2 乗（2^64 倍した整数）のまま、256 ビットの積で厳密に比べる。`M` が 10 なら `100 · dot + 98 · lens ≥ 0` と同じである（[ADR 0027](adr/0027-images-and-figures.md)）。
- `1 j` の継ぎ目は、頂点を中心とする半径 `half` の円板とする。向きが逆になる頂点にも円板を置く。`2 J` の線端は、開いた部分パスと破線の両端を、区間の向きに `half` だけ延ばした butt の線端とする。延ばす量は、法線と同じ丸めで求める。長さ 0 の破線は `2 J` でも描かない。
- `S` は、現在のパスを線の色（`K`、`SC`）で線として描き、パスを空にする（[ADR 0025](adr/0025-strokes-of-paths-with-round-caps-and-dashes.md)）。`h` で閉じた部分パスは閉じたまま、閉じていない部分パスは開いたまま描く。`J`、`j`、`M`、`d` の値はグラフィックスの状態に入り、`q` と `Q` で保存と復元をする。初期値は `0 J`、`0 j`、`10 M`、`[] 0 d` である。`2 Tr` の字の線は、`J`、`j`、`M`、`d` によらず、butt の線端と上限 10 の miter の実線である。
- `1 J` の線は、開いた部分パスと破線の一つ一つの両端に、端点を中心とする半径 `half` の円板を加える。円板は、原点を中心とし、制御点の距離を `k = div_round(half · 36195, 65536)` とする第一象限の四分円の 3 次のベジエ曲線を、曲線と同じ平坦化で線分の列にし、軸で反転して四つの四分円にした多角形である（[ADR 0026](adr/0026-corrections-to-round-caps-and-dashes.md)）。円板は一つの `S` で一度だけ作り、各端点へ平行移動する。
- 点が一つの閉じた部分パス（`m h`）と、すべての点が同じ部分パスは、`1 J` ならその点に円板を描き、`0 J` なら何も描かない。点が一つの開いた部分パス（`m` だけ）は何も描かない（ISO 32000-1 の 8.5.3.2）。
- 破線は、平坦化した装置の空間の線分の列の上で長さを測って切る。長さは、装置の単位の 2^−16 を単位とする整数で、配列の要素と位相は利用者の空間の値に `sqrt(|det CTM|)` を掛けてこの単位に丸め、線分 (a, b) の長さは `isqrt((dx² + dy²) · 2^32)` とする。奇数の要素の配列は二回つないだものとして扱い、周期は二回つないだ配列の和とする（[ADR 0026](adr/0026-corrections-to-round-caps-and-dashes.md)）。部分パスごとに配列の最初から位相（周期で割った余り）だけ進めた位置で始め、「描く」要素の区間を、区間の中の頂点に継ぎ目、両端に線端を付けた開いた部分パスとして線にする。線分の上の距離 t の点は `a + div_round((b − a) · t, L)` とする。長さ 0 の「描く」要素は `1 J` なら円板になる。ただし、部分パスの終点にちょうど重なる長さ 0 の要素は、開いた部分パスでも閉じた部分パスでも描かない（[ADR 0026](adr/0026-corrections-to-round-caps-and-dashes.md)）。閉じた部分パスの最後の破線と最初の破線は継がない。
- 配列の和（周期）が装置の空間で 1/16 画素（2^20 単位）未満なら、`S` を `dash pattern shorter than 1/16 pixel` のエラーにする。
- `S` のパスの平坦化した点の x と y の絶対値に余白 `16 · half + 16` を足した値が `2^31 − 1` を超えれば、`stroke outside the range of coordinates` のエラーにする。これらのエラーは、画素を描かない検査で見つける。
- ここまでの線の規則は、CTM が角を保つ（`a == d && −b == c` か `−a == d && b == c`）ときのものである。角を保たない CTM での線は、利用者の空間で線の多角形を作り、その点を CTM で装置の空間に写して丸める（[ADR 0027](adr/0027-images-and-figures.md)）。利用者の空間の点は Q32.32 の値（2^−32 単位の整数）で持ち、`half` は線の幅の半分、破線の要素と位相は利用者の空間の値のままとする。線端の円板と継ぎ目は利用者の空間の円で作るので、装置の空間では楕円になる。
- 角を保たない CTM での曲線は、利用者の空間で、制御点の二階差分が `2^60 / S` 以下になるまで中点で二分する。`S` は `isqrt(a² + b² + c² + d²) + 1`（Q32.32 の値の整数の平方根に 1 を足した、拡大率の上界）である。キャンバスの外の平坦化は省かない。破線の周期は、`S` を掛けた値が 1/16 画素未満（`周期 · S < 2^60`）なら `dash pattern shorter than 1/16 pixel` のエラーにする。
- 角を保たない CTM での線は、継ぎ目と法線の計算のために、区間の方向の成分を 2^32 未満にシフトする。利用者の空間の点の絶対値が 2^62 以上なら `stroke outside the range of coordinates` のエラーにする。多角形の点を装置の空間に写した値が `2^31 − 1` を超えても同じエラーにする。幅 0 の線は `zero-width stroke with a CTM that does not keep angles` のエラーにする。これらのエラーも、画素を描かない検査で見つける。
- 線分の交点は、キャンバスの行（y が 0 以上、高さ以下）と列（x が 0 以上、幅以下）の中だけを求める（[ADR 0022](adr/0022-no-work-for-geometry-off-the-canvas.md)）。キャンバスの右と上下のセルは捨て、左のセルは行ごとに列 −1 の一つのセルにまとめて cover だけを足す。
- 曲線の二分は、部分の曲線の四つの制御点がすべてキャンバスの上（y が −m 以下）、下（y が高さ + m 以上）、右（x が幅 + m 以上）、左（x が −m − 1 以下）のどれか一つの領域にあれば、そこで止めて部分の曲線を一本の線分にする。余白 `m` は、`0 Tr` の字では 0、`2 Tr` の字では線の幅の半分 `half`（装置の単位）に対して `16 · half + 16` とする。
- ページのパスの `c` の余白は、`c` を読んだときのグラフィックスの状態で決める（[ADR 0025](adr/0025-strokes-of-paths-with-round-caps-and-dashes.md)）。実線（空の配列）で CTM が角を保てば `16 · half + 16`、破線なら平坦化を省かず、CTM が角を保たなければ 0 とする。`f` の画素は余白によらない。
- 破線を切るとき、両端が同じ一つの外の領域（余白は `16 · half + 16`）にある線分は、多角形を作らず、破線の位置だけを進める。その線分で描いている区間を終え、継ぎ目も線端も作らない。
- `2 Tr` の字は、アウトラインのすべての点（制御点を含む）を装置の空間に写した x と y の絶対値に余白 `m` を足した値が `2^31 − 1` 以下でなければ、`glyph stroke outside the range of coordinates` のエラーにする。この検査は描く前に行う。

## 切り抜き

切り抜きは、グラフィックスの状態に入れ、`q` と `Q` で保存と復元をする（[ADR 0027](adr/0027-images-and-figures.md)）。
初期値は、切り抜きなしである。
`W` と `W*` は、直後のパスを終える演算子（`f`、`f*`、`S`、`B`、`B*`、`n`）がパスを描いた後に、そのパスで切り抜きを狭める。
パスは、塗りと同じ覆い（`W` は nonzero、`W*` は偶奇）で 0 から 255 の値にする。
新しい切り抜きの各画素は、今の切り抜きとの積 `div_round(c × p, 255)` とする。
塗りと線は、覆いに切り抜きの値を `div_round(a × c, 255)` で掛けてから合成する。
`2 Tr` の字の線と画像にも切り抜きを掛ける。
Form の `BBox` は、`Do` の中だけ、切り抜きを狭める（[画像と図版](#画像と図版)）。
切り抜きの値は、それが 0 でない画素の外接矩形の中だけに持ち、外接矩形の外は 0 とする。
0 でない画素がなければ、何も描かない。
切り抜きの作業用の領域は、スレッドの `Workspace` で使い回す（[ADR 0013](adr/0013-fewer-allocations-for-musl-threads.md)）。
`Q` で戻った状態より後に作った切り抜きの領域は、次の切り抜きに使う。
切り抜きの計算は、画素を描かない検査では省く。
切り抜きで止まるエラーはない。

## 文字

フォントは、Type 0 のフォントと一つの CIDFont だけを読み、アウトラインは自前で読む。ヒンティングはしない（[ADR 0004](adr/0004-glyph-outlines-cid-mapping-and-vertical-writing.md)）。

- `Encoding` は `/Identity-H` か `/Identity-V` の名前に限る。文字列は 2 バイトずつの CID とし、長さが奇数ならエラーにする。
- `CIDFontType0` は `FontFile3 /Subtype /CIDFontType0C` の裸の CFF を読む。Top DICT に `ROS` があれば charset（形式 0、1、2）で CID からグリフ ID を引き、なければ CID をグリフ ID とする。CID-keyed の CFF は `FDArray` と `FDSelect`（形式 0、3）で局所のサブルーチンを選ぶ。`FontMatrix` は `[0.001 0 0 0.001 0 0]` だけを認める。
- Type 2 charstring は、パスの演算子、`endchar`、サブルーチンの呼び出し、ヒントの演算子（数えるだけ）を解釈する。算術と記憶の演算子、`random`、`seac` の形の `endchar` はエラーにする。スタックは 48、呼び出しの深さは 10 までとする。
- `CIDFontType2` は `FontFile2` の `head`、`maxp`、`loca`、`glyf` を読み、CID をグリフ ID とする。`CIDToGIDMap` は `/Identity` か省略だけを認める。2 次の曲線は 3 次に直す。複合のグリフは、点の合わせと `SCALED_COMPONENT_OFFSET` をエラーにし、入れ子は 8 段までとする。
- 文字の描画の行列は `Trm = [Tfs·Th 0 0 Tfs 0 0] × Tm × CTM` とする。縦組みでは、グリフを位置ベクトル `v` だけ戻して置く。`v` は `W2` か、`DW2`（既定は `[880 −1000]`）の `vy` と `W`（既定は `DW`、1000）の半分の `vx` とする。
- `Tj` の後、字ごとに字送り（横組みは `w0`、縦組みは `w1`）の分だけ `Tm` を進める。
- `0 Tr` はグリフを塗り、`2 Tr` は塗った後に線でなぞる。線の幅は CTM で装置の空間に写す。

## 画像と図版

画像と図版（Form XObject）は、中身を復号し、解釈して描く（[ADR 0027](adr/0027-images-and-figures.md)）。
範囲の外の画像と Form は、画素を描かない検査で見つけ、出力を一つも書かないうちにエラーで止める。
箱で代わりに描くことはしない。
`Do` の名前は、ページか Form の `Resources` の `XObject` から引く。
`Subtype` が `Image` でも `Form` でもない XObject は `unsupported XObject Subtype <値>` のエラーにする。
画像と Form のエラーは、`page <番号>: object <XObject のオブジェクトの番号>: <内容>` の形で示す。

### Form XObject

- `Do` の Form は、`q`、`Matrix`（省略時は単位行列）を CTM に掛ける、`BBox` の矩形で切り抜く、内容を解釈する、`Q` の順に描く（ISO 32000-1 の 8.10.1）。グラフィックスの状態は `Do` のときのものを受け継ぎ、`Do` の後に戻る。
- 内容は、ページの内容と同じ規則で読む。名前は Form の `Resources` から引く。`Resources` のない Form は `a form without Resources`、`BBox` のない Form は `a form without a BBox`、6 個の数でない `Matrix` は `malformed Matrix <値>` のエラーにする。
- Form の中では、`Do` で画像と Form を置ける。入れ子は、ページを 0 段目として 8 段までとし、9 段目の Form は `forms nested deeper than 8` のエラーにする。自身を置く Form も、このエラーで止まる。
- Form の中の `BT` は `text in a form` のエラーにする。
- `Group`、`Ref`、`OC` を持つ Form は `unsupported <キー> <値>`、`FormType` が 1 でない Form は `unsupported FormType <値>` のエラーにする。
- Form の内容のエラーは、`offset` に Form の内容の中のバイトの位置を示す。
- `BBox` の切り抜きは、ほかの切り抜きと同じく、塗りの覆いで縁をアンチエイリアスする。面積が 0 の `BBox` の Form は何も描かない。

### 画像の XObject

- 読む辞書の項目は、`Width`、`Height`、`BitsPerComponent`、`ColorSpace`、`Filter`、`Decode`、`SMask` である。`Type`、`Subtype`、`Length`、`Intent` は読み飛ばす。`ImageMask` と `Interpolate` は `false` だけを認める。ほかの項目（`Mask`、`Name` など）と、`ImageMask` か `Interpolate` の `true` は、`unsupported <キー> <値>` のエラーにする。
- `Width` と `Height` は正の整数とする。`Width × Height × 成分の数` は 2^26 以下とし、超えれば標本を読む前に `image larger than 67108864 samples` のエラーにする。
- フィルタは、なし、`FlateDecode`、`DCTDecode` のどれか一つとする。ほかのフィルタと二つ以上のフィルタは `unsupported Filter <値>`、`DecodeParms` は `unsupported DecodeParms <値>` のエラーにする。
- 色空間は、`DeviceGray`、`DeviceCMYK`、`ICCBased`（`N` が 1、3、4）と、それらを基にする `Indexed` とする。名前の `DeviceRGB` を含むほかの色空間は `unsupported ColorSpace <値>` のエラーにする。`Indexed` の表は文字列で、長さが `(hival + 1) × 基の色空間の成分の数` でなければエラーにする。`hival` は 0 から 255 とする。
- `BitsPerComponent` は、フィルタがなしか `FlateDecode` なら 1、2、4、8、16（`Indexed` は 1、2、4、8）、`DCTDecode` なら JPEG の SOF の精度（8 か 12）とする。ほかの値は `unsupported BitsPerComponent <値>` のエラーにする。
- フィルタがなしか `FlateDecode` の標本は、行ごとに `ceil(Width × 成分の数 × BitsPerComponent / 8)` バイトを取り、行の端はバイトの境目にそろえる（ISO 32000-1 の 8.9.3）。標本のバイトの数が `Height` 行の分と違えば `the image data does not have the size of the image` のエラーにする。伸長はその大きさで打ち切るので、確保するメモリは標本の大きさを超えない。
- `b` ビットの標本 `v` は `div_round(v × 255, 2^b − 1)` で 0 から 255 にする（16 ビットは `div_round(v × 255, 65535)`）。`Decode` は、省略か既定の配列（`[0 1 …]`、`Indexed` では `[0 hival]`）か、すべての組を反転した配列 `[1 0 …]` だけを認め、反転なら 8 ビットにした後に `255 − v` とする。`Indexed` の反転と、ほかの配列は `unsupported Decode <配列>` のエラーにする。
- 8 ビットにした成分は、[色](#色) の変換で RGB にする。DeviceGray の `v` は `(v, v, v)`、`ICCBased` の `N` 3 はそのまま、DeviceCMYK は 0 から 255 のまま `R = 255 − min(255, C + K)` に入れる。`Indexed` は、標本の番号で表を引き、表のバイトを 8 ビットの成分として基の色空間で RGB にする。`hival` を超える番号は `index <番号> out of range of an Indexed color space of hival <hival>` のエラーにする。
- `SMask` は、`ColorSpace` が `DeviceGray`、`BitsPerComponent` が 8、フィルタがなしか `FlateDecode` の画像だけを認める。`Decode` は省略か `[0 1]` だけを認める。`Matte` とほかの項目は `unsupported <キー> <値>`、`DCTDecode` は `unsupported Filter DCTDecode of an SMask` のエラーにする。`SMask` の大きさは、画像と違ってよい。`SMask` のエラーは、`SMask` のオブジェクトの番号で示す。

### JPEG

`DCTDecode` の標本は、自前の JPEG の復号器で読む（[ADR 0027](adr/0027-images-and-figures.md)）。

- ベースライン（SOF0）、拡張の順次（SOF1、ハフマン符号、8 ビットと 12 ビット）、プログレッシブ（SOF2、8 ビットと 12 ビット）を読む。算術符号、ロスレス、階層の符号化は `unsupported JPEG <SOF の番号>` のエラーにする。成分の数は 1、3、4、標本化の係数は 1 から 4 とし、リスタートの間隔（DRI と RSTn）を読む。
- 逆 DCT は、libjpeg の `jidctint.c` の整数の式（`CONST_BITS` 13、`PASS1_BITS` は 8 ビットで 2、12 ビットで 1）と同じ計算で、標本を 0 から `2^精度 − 1` に切り詰める。色差の補間は、標本化が 2×1 と 2×2 の成分では `jdsample.c` の fancy upsampling と同じ式、ほかの比は標本の繰り返しとする。
- 色の変換は、APP14（Adobe）の `transform` が 1 なら YCbCr、2 なら YCCK、0 なら変換なしとする。APP14 がなければ、成分 3 は YCbCr、1 と 4 は変換なしとする。YCbCr から RGB への変換は、`jdcolor.c` の整数の表（`SCALEBITS` 16）と同じ式とする。YCCK は、YCbCr を RGB にした値を `2^精度 − 1` から引いて CMY とし、K はそのままとする。
- 12 ビットの標本は、色の変換の後に `div_round(v × 255, 4095)` で 8 ビットにする。
- 画像の辞書の `Width`、`Height`、`BitsPerComponent` が SOF と合わなければ `<キー> <辞書の値> of the image does not match the JPEG frame (<SOF の値>)`、色空間の成分の数が SOF と合わなければ `the color space of the image has <n> components but the JPEG frame <m>` のエラーにする。
- 壊れた JPEG（途中で切れたもの、表のない符号など）は `corrupt JPEG data: <内容>` のエラーにする。Adobe の反転した CMYK の JPEG は、`Decode [1 0 1 0 1 0 1 0]` で反転してから CMYK の変換に入れる。

### 画像を描く

- 画像は、画像の空間の単位正方形を CTM で写した平行四辺形を、塗りと同じ覆い（nonzero）で描く。縁のアンチエイリアスは、この覆いで決まる。平行四辺形の四隅を装置の空間に写した面積が 0 のとき、または CTM の行列式が 0 のときは、何も描かない。
- 平行四辺形の中の各画素の色は、双線形の補間で求める。画素の中心 `(x + 1/2, y + 1/2)` を CTM の逆で画像の空間に写し、標本の単位の座標 `(u, v)` にする。標本 `(i, j)`（`j` は上の行から数える）の中心は `(i + 1/2, j + 1/2)` とする。逆の写像は、CTM の成分（Q32.32 の値）から有理数として厳密に求め、2^−16 標本の単位に切り捨てる。
- `u − 1/2` と `v − 1/2` の整数部（床）を `i0`、`j0`、端数を `fx`、`fy`（2^−16 単位）とし、四つの標本 `(i0, j0)`、`(i0 + 1, j0)`、`(i0, j0 + 1)`、`(i0 + 1, j0 + 1)` を、画像の縁の外は縁の標本に切り詰めて引く。
- RGB の各成分と不透明度は、`(s00 · (2^16 − fx) + s10 · fx) · (2^16 − fy) + (s01 · (2^16 − fx) + s11 · fx) · fy` を 2^32 で割って丸めた値（同じ近さなら偶数）とする。補間は 8 ビットの RGB にした後の標本で行い、`SMask` の不透明度は `SMask` の大きさで同じ規則で補間する。RGB と不透明度は別々に補間する（不透明度を掛けてから補間することはしない）。
- 画素の不透明度は、覆いに `SMask` の不透明度を `div_round(a × m, 255)` で掛け、さらに切り抜きを同じ式で掛けた値とし、`(c × a + p × (255 − a) + 127) / 255` で重ねる。
- 画像は、塗りと同じく、平行四辺形のうちキャンバスの中の画素だけを求める。行の中の画素の座標は、行の最初の画素の値から 1 画素ごとの差分を厳密に足して進める。CTM の成分の絶対値が 2^57（Q32.32 の生の値）を超えれば `image outside the range of coordinates` のエラーにする（四隅が装置の空間に収まる CTM では起きない）。

### 検査とキャッシュ

- 画像の復号の誤り（壊れた Flate の標本、壊れた JPEG、大きさの不一致、範囲の外の番号）は、描く前の検査で見つける。検査は、画像を最後まで復号して確かめる。
- 復号した画像は、`Session` の中で、オブジェクトの番号ごとに一度だけ作って共有する。共有の大きさは標本の 2^28 バイトまでとし、超える画像は置くたびに復号する。共有するかどうかで、出力は変わらない。`render_page` と `check_page` は、一つのページの中だけで共有する。

## 色

画素は 8 ビットの RGB で、紙は白とする。ICC のプロファイルは使わない（[ADR 0005](adr/0005-device-colors-and-cmyk-conversion.md)）。

- 色の成分は 0 から 1 に切り詰め、`round(x × 255)`（同じ近さなら偶数）で 0 から 255 にする。
- DeviceGray の `G` は `(G, G, G)` とする（ISO 32000-1 の 10.3.2）。
- DeviceCMYK は `R = 255 − min(255, C + K)`、`G = 255 − min(255, M + K)`、`B = 255 − min(255, Y + K)` とする（10.3.5）。
- RGB は、各成分を同じ `round(x × 255)` で 0 から 255 にする（[ADR 0027](adr/0027-images-and-figures.md)）。
- `ICCBased` の色空間は、プロファイルを読まず、ストリームの `N` が 1 なら DeviceGray、3 なら RGB、4 なら DeviceCMYK と同じ変換で RGB にする（[ADR 0027](adr/0027-images-and-figures.md)）。`Alternate` と `Range` は読まない。`N` がほかの値なら `unsupported ICCBased with N <値>`、配列が `[/ICCBased ストリーム]` の形でなければ `malformed ICCBased color space` のエラーにする。
- Separation は、代替の色空間が DeviceCMYK か DeviceGray で、変換が `N` 1 の `FunctionType 2` のものだけを読む。濃度 `t` を `Domain` に切り詰め、`C0 + t × (C1 − C0)` を代替の色空間で RGB にする。`cs` の後の色は濃度 1 とする。色材の名前は使わない。
- `cs` は塗りの色空間を、`CS` は線の色空間を、ページの `Resources` の `ColorSpace` にある `ICCBased` か Separation にする。`ICCBased` を選んだ後の色は黒とする。
- `k` は塗りの色を、`K` は線の色を DeviceCMYK にする。`sc` は塗りの色を、`SC` は線の色を、その色空間の成分の数の数で指定する。色空間が `ICCBased` か Device のときだけ認め、成分の数が違えば `<演算子> with <数> components for a color space of <成分の数>`、Separation なら `unsupported <演算子> with a Separation color space` のエラーにする。`scn` は塗りの色空間が Separation のときだけ認める。色と色空間の初期値は DeviceGray の黒で、`q` と `Q` で保存と復元をする。
- ほかの色空間（名前の DeviceRGB、Indexed など）と、ほかの関数はエラーにする。

## 決定性

同じ入力と同じオプションから、x86-64 と aarch64、Linux と macOS で、バイト単位で同じ出力を出す（[ADR 0001](adr/0001-scope-golem-contract-and-determinism.md)）。

- 入力から画素までの経路で、浮動小数点数を使わない。CI は `src/` に `f32` と `f64` がないことを `scripts/ci/check-no-float.sh` で確かめる。
- 実数と行列の要素は Q32.32（符号付き 64 ビットの整数、小数部 32 ビット）で表す。
- 十進の数の文字列は、桁数によらず正確に、最も近い Q32.32 の値に丸める（同じ近さなら偶数）。整数部の絶対値は 2^31 未満とし、超えればエラーにする。
- 掛け算、割り算、行列の積と点の変換は、128 ビットの整数で計算してから一度だけ丸める（最近接の偶数への丸め）。平方根は切り捨てる。
- 桁あふれはエラーにする。黙って折り返さない。

速さの記録は `docs/benchmarks/` に置く（[2026-10-27-parallel-render.md](benchmarks/2026-10-27-parallel-render.md)、[2026-10-27-speed-up.md](benchmarks/2026-10-27-speed-up.md)、[2026-10-27-musl-threads.md](benchmarks/2026-10-27-musl-threads.md)、[2026-11-05-thread-workspaces.md](benchmarks/2026-11-05-thread-workspaces.md)、[2026-11-05-faster-png-render-and-decoding.md](benchmarks/2026-11-05-faster-png-render-and-decoding.md)）。フォントとグリフのアウトラインの使い回しなど、出力のバイト列を変えない高速化は、仕様の振る舞いを変えない（[ADR 0011](adr/0011-speed-up-without-changing-bytes.md)）。

musl のメモリ割り当ては一つの錠で守られ、割り当てのたびにスレッドが待ち合う。
そこで、ページの検査と描画の割り当ての回数は、グリフ、線分、曲線の数に比例させない（[ADR 0013](adr/0013-fewer-allocations-for-musl-threads.md)）。

- パスは、すべての点を一つの `Vec` に入れ、グリフのパス、線の多角形、塗りの作業用の場所（`raster::Scratch`）は、ページを描く間は使い回す。
- 内容のストリームの字句のキーワードと、`#` のない名前は、元のバイト列のスライスとして扱う。演算子 `cs`、`Tf`、`Do` の名前も、内容のバイト列を借りる。
- `shem render` の PPM は、画素を間に別の `Vec` を作らずにファイルへ書く。
- 結合テスト `tests/allocations.rs` が、同じグリフの 101 個と 1 個の割り当ての差がグリフあたり 4 回以下であることと、長方形、線分、曲線の数に比例しないことを確かめる。
- 結合テスト `tests/kanikosen.rs` が、Golem の kanikosen の fixture のすべてのページを 72 dpi と 150 dpi で描き、PNG と PPM のバイト列が v0.1.0-rc2 と同じであることを確かめる。
- 速さは、手動で起動するワークフロー Benchmark（`.github/workflows/benchmark.yml`）で、x86-64 の musl のビルドを ubuntu-24.04 で測る。

その後、残った割り当てと、スレッドの数によらない CPU の時間を減らした（[ADR 0014](adr/0014-thread-workspaces-and-faster-check-and-fill.md)）。どれも出力のバイト列とエラーを変えない。

- 字句は、16 進の文字列をスライスのまま返し、`Tj` の文字列は描くときに解きながら読む。
- CFF の charstring の引数のスタックは長さ 48 の固定の配列で、グリフのアウトラインはスレッドのバッファに解釈してから、キャッシュへ一度だけ写す。結合テスト `tests/allocations.rs` が、初めて解釈するグリフの割り当てがグリフあたり 4 回以下であることを確かめる。
- スレッドは、検査と描画のすべてのページで一つの `Workspace` を使い回す。
- 十進の数は、小数部が 19 桁以下なら一度の割り算と余りで丸める。20 桁以上は、小数部を 2 倍しては繰り上げて読む。どちらも、最も近い Q32.32 の値（同じ近さなら偶数）になる。
- W と W2 は、範囲が昇順で重ならなければ二分探索で引き、そうでなければ先頭からたどる。どちらも、CID を含む最初の範囲を使う。
- 画素を描かない検査では、グリフの点をデバイス空間に写して桁あふれを確かめるだけで、パスと線の多角形は作らない。
- 塗りでは、パスの点の外接矩形から、画素を変えうるセル（画像の中の行と、列 −1 から画像の右端までの列）を求める。その数が 2^16 以下なら、セルを密な格子に足し込み、超えれば並べ替えて同じセルを足す。画像の左のセルは列 −1 にまとめ、右と上下のセルは捨てる。

さらに、PNG の書き出し、描画、Flate の展開を速くした（[ADR 0015](adr/0015-faster-png-render-and-decoding.md)）。これも出力のバイト列とエラーを変えない。

- PNG の行のフィルタは、5 種類を番号の順に試し、費用が 0 の種類が出たら打ち切る。どの種類も、最初の画素とそれ以降を別々のループで計算する。
- Deflate は、固定の Huffman の符号と、長さと距離の符号を表から引く。候補の、今の最長の一致の長さの位置のバイトが違えば、その候補の長さは数えない。同じバイトの連続の中の候補は、連続の長さから一致の長さが決まるので、まとめて式で処理する。どちらも、候補を試した回数（128 まで）には数える。一致の後に連鎖に足す位置も、同じバイトの連続はまとめて足す。
- Adler-32 は 16 本の列で、CRC-32 は 8 バイトずつの表で計算する。
- 十進の数は、小数部が 9 桁以下なら u64 で割って丸める。整数で割る `Fixed::checked_div` は i64 で割る。どちらも値は同じである。
- 字句は、バイトの種類を表で引き、語の終わりと数の形を 1 回の走査で調べる。整数は文字列に直さずに読む。
- 検査では、グリフの座標の絶対値の最大と行列から、写した点が桁あふれしないことを上限で示せれば、点を写さない。描画では、示せたグリフの点を、同じ二度の丸めで、桁あふれの検査なしで写す。示せなければ、一つずつ検査して写す。
- 線分と行や列との交点は、積が i64 に収まれば i64 で割る。密な格子は、行ごとに左から足しながら、画素を直接画像に重ねる。曲線は再帰で分割する。
- Flate の Huffman の符号は、11 ビットまでを表で引き、それより長い符号とデータの終わりの近くでは 1 ビットずつ読む。

さらに、Deflate の探索、密な格子の掃き出し、演算子の数を速くした（[ADR 0019](adr/0019-faster-deflate-sweep-and-operands.md)）。これも出力のバイト列とエラーを変えない。

- Deflate は、探索で数えた今の位置の同じバイトの連続とハッシュを、探索の後の位置の追加に使う。連続と一致の長さの 8 バイトに満たない残りは、等しいとわかっているバイトに重ねた 8 バイトの語一つで比べる。
- PNG の行のフィルタの費用は、2^24 バイト以下の行では u32 で足す。Adler-32 の 16 本の列は、256 個の塊ごとに u16 で足してから畳む。
- 線分と行や列との交点は、最初の交点だけを割り、次からは商と余りを持ち越す。値と丸めは、交点ごとに i128 で割るものと同じである。座標の差が小さい線分（|db| ≤ 2^54、d ≤ 2^61）は i64 で、ほかは i128 で持ち越すので、i64 のどの座標でも正しい（[ADR 0020](adr/0020-carried-crossings-for-every-i64.md)）。
- 密な格子は、行ごとに、最初と最後に足された場所の間だけを掃く。その右の画素は、行の覆いの累積から一つの範囲として出す。掃いた場所は塗りの後で 0 に戻すので、格子は塗りの間で 0 である。黒の塗りは、色の項を省いた同じ式で重ねる。
- 内容の字句の数は、オブジェクトに包まずにオペランドのスタックに置く。

キャンバスの外の線分と曲線には、座標の幅に比例する時間をかけない（[ADR 0022](adr/0022-no-work-for-geometry-off-the-canvas.md)）。
これも出力のバイト列を変えない。
画素が変わらないのは、キャンバスの外のセルが画素に効く形が三つに限られるからである。
行の外（0 未満か高さ以上）のセルと右（幅以上の列）のセルはどの画素も変えず、左（0 未満の列）のセルはその行の cover に足されるだけで area は使われない。

- 線分は、キャンバスの行の中の交点だけを求める。端が行の外にある区間は行の外のセルだけを作るので、渡さない。範囲の最初の交点の座標は、その交点の積を割って求め、次からは商と余りを持ち越す（[ADR 0020](adr/0020-carried-crossings-for-every-i64.md)）。最初の交点が線分の始点から遠いと積が i64 を超えうるので、そのときは積だけを i128 で割る。値は交点ごとに割るものと同じである。
- 行の中の区間は、キャンバスの列の中の交点だけを求める。左の部分のセルの cover の和は、隣り合うセルの y の差の和なので、両端の y の差に等しい。これを列 −1 の一つのセルとして足す。
- 曲線の二分で、部分の曲線の制御点がすべて一つの領域にあれば、その部分の曲線を一本の線分にする。中点は両端の間にあるので、二分で生まれる点はすべて制御点の座標の範囲に収まり、置き換える前の線分の列も置き換えた線分も同じ領域にある。上、下、右の領域の線分は画素を変えず、左の領域では行に足す cover の和が始点と終点の y だけで決まるので、どちらも同じである。
- `2 Tr` の字の線の多角形は、区間の四角形が区間から `half`、継ぎ目の多角形が頂点から `16 · half` と数単位（[ADR 0023](adr/0023-miter-points-within-the-limit.md)）の範囲にあるので、余白 `16 · half + 16` の中にある。左の領域の多角形は閉じているので、行に足す cover の和は 0 である。余白を足しても i32 に収まる字だけを描くので、多角形の点は折り返さない。

## 配布

`v` で始まるタグを push すると、Linux の x86-64 と aarch64（musl で静的にリンク）と macOS の aarch64 のアーカイブ `shem-<タグ>-<ターゲット>.tar.gz` と `SHA256SUMS` を、GitHub の Releases に置く（[ADR 0001](adr/0001-scope-golem-contract-and-determinism.md)）。
同じリリースに、wasm のモジュール `shem.wasm` も置く。名前は版によらず同じで、`SHA256SUMS` にも入る（[ADR 0018](adr/0018-wasm-module-for-the-browser.md)）。
タグと `Cargo.toml` の版が合わないか、タグのコミットが main の祖先でなければ、リリースは失敗する（[ADR 0024](adr/0024-provenance-and-pinning-for-ci-and-releases.md)）。

リリースの成果物（三つのアーカイブと `shem.wasm`）には、リリースのワークフローで、ビルドの出自の attestation を付ける。
`gh attestation verify <ファイル> -R tett23/shem` で確かめられる。
`SHA256SUMS` は同じリリースに置くので、取得の破損を確かめるものであり、リリースの書き換えは防がない。
リポジトリの immutable releases を有効にしているので、公開したリリースの資産とタグは変えられない。
`v` で始まるタグには ruleset があり、付け替えと削除を禁じる（bypass できる者はいない）。

CI、リリース、benchmark のワークフローは、`rust-toolchain.toml` の版の Rust（1.99.1）でビルドする。
benchmark は、比べるすべての ref を、ワークフローを動かしたコミットの `rust-toolchain.toml` の版でビルドする。
ワークフローの action はコミットの SHA で参照し、リポジトリの設定でもそれを強制する。

## 文書の翻訳

README.md と CONTRIBUTING.md は英語を正とし、README.ja.md と CONTRIBUTING.ja.md をその機械翻訳とする（[ADR 0008](adr/0008-english-canonical-readme-and-contributing.md)）。

- 英語版を変えるコミットは、同じコミットで対応する翻訳も変える。翻訳だけを変えるコミットは認める。
- コミットメッセージの最後の段落に `Translation-Unchanged: <理由>`（キーの大文字と小文字は区別しない。理由は空でない）があれば、翻訳を変えなくてよい。
- 検査は `scripts/check-translations.sh` が行う。git の commit-msg の hook はステージされた変更を、CI は範囲のマージ以外の各コミットを検査する。違反は `check-translations: <コミット>: <英語版> changed without <翻訳> (...)` を出し、終了コード 1 で終わる。使い方の誤りは終了コード 64 で終わる。

## ADR の検査

ADR の検査には adr-guard の v0.10.0 を使う（[ADR 0017](adr/0017-update-adr-guard-to-v0.10.0.md)）。

- git の pre-commit の hook と CI は、コミット済みの ADR がステータスの行とリンクを除いて変わらないことを検査する。
- CI は、`docs/adr/` で番号が重複した ADR がないことも検査する。
- 文書の ADR の参照には、`adr-guard links --fix` でリンクを付ける。ほかのリポジトリの ADR を指すときは、外部リンクの中に書く。

## 破棄した ADR

なし。
