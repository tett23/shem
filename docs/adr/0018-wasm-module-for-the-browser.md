# ADR 0018: ブラウザで動かす wasm のモジュールを作り、リリースに添付する

ステータス: 採択

## 文脈

tett23/tett23 の GitHub Pages に、Golem のページを作る（[tett23/tett23 の ADR 0005](https://github.com/tett23/tett23/blob/68c4f87004b0629c1dffc3c1ff23c751853721dd/.github/docs/adr/0005-golem-page.md)）。ページのプレイグラウンドは、ブラウザの Web Worker の中で、Golem の wasm で PDF を組み、shem の wasm でページごとにラスタライズして、144 DPI の右綴じの見開きで表示する。この ADR は、その 2 段目の wasm を決める。tett23/tett23 のセッションからの依頼（2026-11-07）による。

受け入れる側は [tett23/tett23 の ADR 0002](https://github.com/tett23/tett23/blob/68c4f87004b0629c1dffc3c1ff23c751853721dd/.github/docs/adr/0002-product-pages-on-github-pages.md) で、次のことを決めている。

- wasm は各プロダクトのリポジトリの CI でビルドし、GitHub Releases に添付する。ページ側では wasm をビルドしない。
- ページ側は `wasm.lock` に `名前 URL SHA-256` を書いて取り込む。添付するファイルの名前は版をまたいで変えない。
- Rust のものは `wasm32-unknown-unknown` でビルドし、`extern "C"` の関数で、線形メモリ上のポインタと長さを受け渡す。wasm-bindgen は使わない。
- ブラウザ（Safari と Chrome の最新版）の Web Worker と Deno で、wasm を読み込むだけで動く。import は空にする。

### 試作で確かめたこと

shem の main（[8524d8d](https://github.com/tett23/shem/tree/8524d8d84986e16f8009b64f9fa84dd7fa3f8fdf)）で、次のことを確かめた。

- ライブラリは、変更なしで `wasm32-unknown-unknown` でビルドできる（rustc 1.97.0-nightly）。`std::fs` を使うのは CLI だけで、ライブラリのページの描画はスレッドを作らない（`render_page_with`、[src/render.rs:492](https://github.com/tett23/shem/blob/8524d8d84986e16f8009b64f9fa84dd7fa3f8fdf/src/render.rs#L492)）。`parallel::available_jobs` は、wasm では `available_parallelism` が使えないので 1 を返す（[src/parallel.rs:81](https://github.com/tett23/shem/blob/8524d8d84986e16f8009b64f9fa84dd7fa3f8fdf/src/parallel.rs#L81)）。
- 作業用の crate（shem をパスで依存に入れ、`crate-type = ["cdylib"]`）で、入力の場所を確保する関数と、1 ページを描いて RGBA にする関数をエクスポートした。Deno 2.9.7 で `WebAssembly.Module.imports` を見ると空だった。大きさは 404 KB だった。
- kanikosen の 1 ページ目を 144 DPI で描くと、ページの数は 129、大きさは 726 × 1032 だった（JIS の B6 の 128 × 182 mm を 144 DPI にすると 725.67 × 1031.81 で、切り上げ）。RGBA の R、G、B は、ネイティブの `shem render --dpi 144 --format ppm` の画素とバイト単位で一致した。描く時間は 83.5 ミリ秒だった（Apple M1 Max）。
- wasm32-unknown-unknown では、パニックはトラップになり、エラーとして返せない。試作に、範囲の外を読んでパニックする関数を足して Deno で呼ぶと、`WebAssembly.RuntimeError`（`unreachable`）が投げられた。この試作では、その後も同じインスタンスの関数を呼べたが、パニックした関数の途中の状態（借りたままの `RefCell` など）が残りうる。そこで、sample、figures、truetype、sample-marks の PDF を切り詰めたもの、1 ビットを反転したもの、数バイトを置き換えたもの、合わせて 1,600 個を、ネイティブで読み、最初の 3 ページを描いた。パニックは一つもなかった。

## 実装すること

### エクスポートする関数

- shem のライブラリに `wasm` のモジュールを足し、次の関数を `#[unsafe(no_mangle)] pub extern "C"` でエクスポートする。エクスポートは `target_arch = "wasm32"` のときだけ付ける。
- 中身は、ネイティブでもテストできる普通の関数（モジュールの状態を引数に取る）に置き、エクスポートはそれを呼ぶだけにする。
- 引数と戻り値は、どれも `u32`（ポインタ、長さ、番号）か `i32` とする。

| 関数 | 引数 | 戻り値 | 意味 |
|---|---|---|---|
| `shem_alloc` | `len` | ポインタ | `len` バイトの入力の場所を確保して、0 で埋める |
| `shem_free` | `ptr` | なし | `shem_alloc` の場所を解放する |
| `shem_open` | `ptr`、`len`、`dpi` | 文書の番号（1 以上）、失敗なら 0 | 入力の PDF を読み、ページの木と内容を読む |
| `shem_page_count` | `doc` | ページの数、番号が違えば 0 | |
| `shem_render` | `doc`、`page` | 成功なら 1、失敗なら 0 | 0 から数えたページを描き、文書の画像にする |
| `shem_image_ptr` | `doc` | RGBA の先頭のポインタ | 最後に描いた画像 |
| `shem_image_width` | `doc` | 幅（画素） | |
| `shem_image_height` | `doc` | 高さ（画素） | |
| `shem_close` | `doc` | なし | 文書と画像を解放する |
| `shem_error_ptr` | なし | 最後のエラーの文の先頭のポインタ | UTF-8 |
| `shem_error_len` | なし | 最後のエラーの文のバイト数 | |

- 入力の場所は、モジュールが `Vec<u8>` として持ち、ポインタで引く。ページ側は `shem_alloc` の返したポインタから `len` バイトに PDF を書いてから、`shem_open` を呼ぶ。`shem_open` は入力を写して持つので、ページ側はその後で `shem_free` してよい。
- `shem_open` に、`shem_alloc` で確保していないポインタか、確保した長さを超える長さを渡すと、失敗にする。ポインタは `Vec` の先頭のアドレスで、モジュールは自分の持つ `Vec` の中から探すので、生のポインタを読む `unsafe` のコードは要らない。
- 画像の RGBA は、shem の RGB に不透明度 255 を足したもので、行は上から、画素は左からの順である。長さは幅 × 高さ × 4 バイトになる。画像は、同じ文書の次の `shem_render` か `shem_close` まで有効である。ページ側は `new ImageData(new Uint8ClampedArray(memory.buffer, ptr, len).slice(), width, height)` のように写して使う。
- 1 枚目から先に表示できるよう、ページの数を先に返し、ページを 1 枚ずつ描く。`shem_render` は、描くページだけを検査して描く。ほかのページは検査しない（`shem render` のように、すべてのページを先に検査することはしない）。
- フォントは、文書ごとのキャッシュ（[ADR 0011](0011-speed-up-without-changing-bytes.md)）で、ページの間で使い回す。

### エラー

- 失敗した関数は 0 を返し、エラーの文を、次の失敗まで `shem_error_ptr` と `shem_error_len` で読めるようにする。
- 文は `shem render` が標準エラーに書く文の、`shem: <パス>: ` の後と同じである（例：`page 3: object 12: offset 4567: unsupported operator sh`）。番号の違う文書、範囲の外のページ、範囲の外の DPI、確保していない入力のときは、その旨の英語の文にする。
- DPI は `shem render` と同じく 1 以上 1200 以下とする（[ADR 0007](0007-png-output-resolution-and-cli.md)）。ページの画素の数の上限（2^28）も同じである。
- パニックはトラップになるので、エラーとしては返せない。トラップしたインスタンスは、状態が途中で止まっていることがあるので、捨てて読み込みなおす必要がある。どの入力でもパニックしないことを、下のテストで確かめる。

### ビルドと添付

- `Cargo.toml` は変えない。wasm は `cargo rustc --release --locked --lib --target wasm32-unknown-unknown --crate-type cdylib` でビルドする。
- CI に `wasm` のジョブを足す。wasm をビルドし、Deno で下のテストを行う。Deno は公式の setup のアクション（Node 24 で動く最新のメジャー版、コミットのハッシュで固定）で入れる。
- リリースのワークフローに、wasm をビルドして `shem.wasm` の名前で添付する手順を足す。名前は版をまたいで変えない。`SHA256SUMS` にも入れる。
- README（英語版と日本語版）の Installation と Usage に、`shem.wasm` と、エクスポートした関数の一覧と使い方を書く。仕様に「wasm のモジュール」の節を足す。
- リリースの後、タグ、添付したファイルの URL と SHA-256、エクスポートした関数の一覧と入出力の形を、tett23/tett23 のセッションに伝える。
- 出力を変えないので、リリースはプレリリースの版を上げる（[ADR 0001](0001-scope-golem-contract-and-determinism.md)）。

## 実装しないこと

- PNG の符号化を wasm でエクスポートすること。ページ側は RGBA を受け取る。
- wasm での並列化（Web Worker や wasm のスレッド）。ページは 1 つのスレッドで描く。
- wasm-bindgen、wasm-pack、WASI。import は空にする。
- `shem diff` と、すべてのページを先に検査することの wasm へのエクスポート。
- wasm の大きさを縮める処理（`wasm-opt` など）。依存を増やさない。大きさが問題になってから考える。
- Safari と Chrome での自動のテスト。このリポジトリの CI では Deno で確かめ、ブラウザでの確かめはページ側のテストに任せる。

## テスト設計

### ネイティブのテスト（Rust）

- モジュールの関数を、ネイティブで直接呼ぶ。
  - sample と kanikosen を、確保、書き込み、`shem_open`、ページごとの `shem_render` の順で描くと、ページの数が `Session` と同じで、RGBA の R、G、B が `Session::render` の画素と同じで、不透明度がすべて 255 である。
  - 番号の違う文書、範囲の外のページ、0 と 1201 の DPI、確保していないポインタ、確保した長さを超える長さで、0 が返り、エラーの文が決めたとおりになる。閉じた文書の番号も、番号の違う文書として扱う。
  - 失敗しても、それまでに開いた文書と画像は使える。
- 壊れた PDF：sample、figures、truetype、sample-marks、kanikosen の PDF を、切り詰めたもの、1 ビットを反転したもの、数バイトを置き換えたもの（固定の種で、合わせて 5,000 個）を `shem_open` で読み、開けたものはすべてのページ（kanikosen は最初の 5 ページ）を `shem_render` で描く。パニックせずに、成功か 0 とエラーの文が返る。パニックが見つかれば、その場所を直す。

### wasm のテスト（Deno、CI の `wasm` のジョブ）

- `WebAssembly.Module.imports` が空で、`WebAssembly.instantiate(module, {})` で読み込める。エクスポートが上の表の関数と `memory` である。
- kanikosen を 144 DPI で開くと、ページの数が 129 で、すべてのページの大きさが 726 × 1032（JIS の B6 を 144 DPI にした大きさ）である。
- すべてのページの RGBA の R、G、B が、同じジョブでネイティブの `shem render --dpi 144 --format ppm` が書いた PPM の画素とバイト単位で一致し、不透明度がすべて 255 である。
- 壊れた PDF（切り詰めたもの、PDF でないバイト列）を渡すと、トラップせずに 0 が返り、エラーの文が空でない。
- 同じ文書でページを順と逆順で描いても、同じ画像になる。

## トレードオフ

- ページを 1 枚ずつ描くので、1 枚目から表示できる。そのかわり、ほかのページが範囲の外の入力でも、そのページを描くまでわからない。`shem render` の「すべてのページを検査してから書く」とは振る舞いが違う。
- エクスポートの形をポインタと番号の整数にしたので、import が空になり、ページ側の受け渡しのコード（tett23/tett23 の `lib/`）で扱える。そのかわり、ページ側は線形メモリから画像を写す必要がある。
- パニックをエラーにできないので、どの入力でもパニックしないことを、壊れた PDF のテストで確かめ続ける必要がある。
- wasm のモジュールをライブラリに入れるので、ネイティブのビルドにもそのモジュール（エクスポートの付かない普通の関数）が入る。

## 参考文献

- tett23/tett23 のセッションからの依頼「引き継ぎ：shem のラスタライズを wasm にする」（2026-11-07）。リポジトリにはコミットされていない。
- [tett23/tett23 の ADR 0002「プロダクトのページを GitHub Pages で公開する基盤」](https://github.com/tett23/tett23/blob/68c4f87004b0629c1dffc3c1ff23c751853721dd/.github/docs/adr/0002-product-pages-on-github-pages.md)
- [tett23/tett23 の ADR 0005「Golem のページとプレイグラウンド」](https://github.com/tett23/tett23/blob/68c4f87004b0629c1dffc3c1ff23c751853721dd/.github/docs/adr/0005-golem-page.md)
- The Rust Reference, "The `no_mangle` attribute"（Rust 2024 では `unsafe(no_mangle)` と書く） https://doc.rust-lang.org/reference/abi.html#the-no_mangle-attribute
- Cargo Book, "cargo rustc"（`--crate-type`） https://doc.rust-lang.org/cargo/commands/cargo-rustc.html
- MDN, "WebAssembly.Module.imports()"、"ImageData()" https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/Module/imports_static
