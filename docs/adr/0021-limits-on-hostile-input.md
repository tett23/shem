# ADR 0021: 入れ子、フォントの解釈、Flate の伸長、差分の画像に上限を置く

ステータス: 採択

## 文脈

2026-11-08 の脆弱性診断（[docs/audit/2026-11-08-vulnerability-assessment.md](../audit/2026-11-08-vulnerability-assessment.md)）で、小さな入力でプロセスを止めるか、時間かメモリを使い尽くさせる DoS が見つかった。
この ADR は、そのうち次の五つを扱う。
キャンバスの外の図形にかかる時間（所見 6）は、出力のバイト列にかかわる計算なので、[ADR 0022](0022-no-work-for-geometry-off-the-canvas.md) で扱う。

- 配列と辞書の入れ子（所見 1）と、ページの木の深さ（所見 2）に上限がなく、スタックがあふれてプロセスが落ちる。
- CFF のサブルーチンの呼び出し（所見 3）と TrueType の複合グリフ（所見 4）は、深さだけを制限し、総数を制限しない。一つのグリフの解釈が、フォントの大きさに対して指数的に増える。
- Flate の伸長（所見 5）に大きさの上限がなく、255 KB の PDF で 517 MB を確保する。
- `shem diff`（所見 7）は、一枚の画像を 2^28 画素までに制限しているが、差分の画像の大きさは二枚の幅と高さの掛け算で決まる。
- wasm の `shem_alloc`（軽微なもの）は、確保に失敗するとエラーの文を返せないトラップになる。

[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md) は、範囲の外の入力をエラーで止めると決めた。
上限を超える入力も範囲の外として扱えば、その原則のまま、描く前の検査で止められる。

上限の値を決めるため、fixture の五つの PDF を、計測用の印を入れた一時的なビルドで 72 dpi と 300 dpi で描き、各値の最大を測った（2026-11-08、印を入れたビルドはコミットしていない）。

| 値 | fixture の最大 | 上限 |
|---|---|---|
| 配列と辞書の入れ子の深さ（オブジェクトと内容のストリーム） | 6（sample-marks.pdf） | 64 |
| ページの木の深さ（`Pages` と `Page` の段の数） | 2 | 64 |
| CFF の一つのグリフの解釈の段数（数と演算子を一つずつ数える） | 827（kanikosen.pdf） | 65,536 |
| CFF の一つのグリフの charstring の実行の回数（本体とサブルーチン） | 72（kanikosen.pdf） | 段数の上限に含める |
| TrueType の一つのアウトラインが読むグリフの数（自身と部品） | 2（truetype.pdf） | 256 |
| 一つのストリームの伸長の後の大きさ | 675,123 バイト（kanikosen.pdf のフォント） | 2^26 バイト（64 MiB） |

どの上限も、fixture の最大の 10 倍以上にしてある。
v1 で汎用の PDF を読むときに足りなくなれば、別の ADR で上げる。

## 実装すること

### 配列と辞書の入れ子

- `object_from`（[src/pdf/lexer.rs:391](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/pdf/lexer.rs#L391)）に入れ子の深さを渡し、64 を超える `[` か `<<` に出会ったら `arrays and dictionaries nested deeper than 64` のエラーにする。位置は、その `[` か `<<` のバイトの位置とする。
- trailer、オブジェクトの本体、内容のストリームの被演算子は、どれも同じ関数を通るので、同じ上限になる。

### ページの木

- `walk`（[src/pdf.rs:355](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/pdf.rs#L355)）に深さを渡し、根の `Pages` を 1 段目として 64 段を超えたら `page tree nested deeper than 64` のエラーにする。オブジェクトの番号は、65 段目のノードのものとする。
- 循環の検査（`visited`）はそのまま残す。

### CFF の解釈の段数

- `Interpreter`（[src/font/cff.rs:378](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/font/cff.rs#L378)）に、一つのグリフで解釈した数と演算子の数を数える欄を足す。サブルーチンの中のものも、同じ欄に数える。
- 65,536 を超えたら `charstring longer than 65536 steps` のエラーにする。
- 呼び出しの深さの上限（10）と、引数のスタックの上限（48）は変えない。

### TrueType の複合グリフ

- `glyph`（[src/font/truetype.rs:226](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/font/truetype.rs#L226)）が読むグリフの数を、一つのアウトラインごとに数える。数は、`outline` が最初に読むグリフを 1 とし、部品を読むたびに 1 を足す。
- 256 を超えたら `composite glyph of more than 256 glyphs` のエラーにする。
- 入れ子の深さの上限（8）は変えない。

### Flate の伸長

- `inflate_zlib` に、伸長の後の大きさの上限を引数で渡す。上限を超えるバイトを書こうとした時点で `decoded data larger than <上限> bytes` のエラーにする。上限まで伸長してから確かめるのではなく、超える前に止める。
- PDF のストリーム（`stream_data`、[src/pdf.rs:294](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/pdf.rs#L294)）は、上限を 2^26 バイトとする。ページの内容の配列は、ストリームごとに上限を当てる。
- `shem diff` の PNG（[src/image.rs:93](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/image.rs#L93)）は、上限を `IHDR` から決まる大きさ `height × (width × 3 + 1)` とする。超えたときのエラーの文は、今の大きさの不一致の文（`malformed image: the image data does not have the size of the image`）と同じにする。
- 参照の実装（`src/flate/reference.rs`）は、テストの比較の相手なので変えない。

### 差分の画像

- `diff`（[src/diff.rs:36](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/diff.rs#L36)）は、差分の画像の幅と高さを決めた後、画素の数が 2^28 を超えれば、画素を作る前に `the diff image is too large: <幅> × <高さ> pixels` のエラーを返す。
- `shem diff` は、それを `<後>` の画像のエラーとして、終了コード 2 で `shem: <後のパス>: <内容>` と書く。ディレクトリを比べるときは、名前の順で最初のエラーを報告する今の規則に従う。

### wasm の確保

- `Module::alloc`（[src/wasm.rs:53](https://github.com/tett23/shem/blob/4ab16a6deb19ec403aa438b5e7540a5eec6fd0f6/src/wasm.rs#L53)）は、`Vec::try_reserve_exact` で確保する。失敗したら、`cannot allocate <len> bytes` をエラーの文にして 0 を返す。
- `shem_alloc` の戻り値 0 は、失敗を表す。確保した場所のポインタは 0 にならない。

### 文書

- 仕様の「読む PDF の部分集合」に、入れ子とページの木の上限、フォントの解釈の上限、伸長の上限を書く。「差分の画像」に差分の画像の上限を、「wasm のモジュール」に `shem_alloc` の失敗を書く。
- README の wasm の節に、`shem_alloc` が 0 を返したら `shem_error_ptr` と `shem_error_len` で文を読めることを書く（README.ja.md も訳しなおす）。

## 実装しないこと

- 文書全体の伸長の大きさの上限。`Document::pages_with` はページの内容を並列に伸長して持つので、文書全体の上限を当てると、どのページで超えるかがスレッドの終わる順で変わり、エラーが決定的でなくなる。ストリームごとの上限だけでは、メモリの量は入力の大きさの約 1,000 倍（Deflate の圧縮の比の上限）まで増えうる。包むサービスは、入力の大きさを制限する。
- ページの内容を描くまで伸長しない形にすること。描く前にすべてのページを検査する決まり（[ADR 0002](0002-pdf-subset-and-out-of-scope-input.md)）のため、内容を二度伸長することになる。
- 一つのページの描画の時間の上限。塗りの時間は、塗る回数とキャンバスの大きさの積に比例する。これはキャンバスに実際に描く量で、ラスタライザに共通の性質である。包むサービスは、時間の上限（タイムアウト）と `--dpi` の上限を置く。
- `--jobs` の上限を下げること。今の上限（1024）とページの大きさの上限（2^28 画素）の積は大きいが、どちらも利用者の指定である。
- 線の多角形の座標の折り返し（診断の軽微なもの）。再現していない仮説なので、ここでは扱わない。`Tr` が 2 の字の線の点の範囲は、[ADR 0022](0022-no-work-for-geometry-off-the-canvas.md) が検査に加える。
- 上限を設定で変えられるようにすること。

## テスト設計

どのテストも、実装の前に書き、今のコードで失敗する（エラーにならない、または上限を確かめられない）ことを確かめてから実装する。

### 入れ子

- trailer、オブジェクトの本体、内容のストリームのそれぞれに、深さ 64 の配列（と辞書）を置いた PDF は通る。深さ 65 なら、文のとおりのエラーになり、位置が 65 個目の `[` か `<<` を指す。
- 深さ 10 万の `[` を並べた PDF（数十 KB）と、内容のストリームに 10 万個の `[` を置いた PDF で、`shem render` が終了コード 1 でエラーの文を書く。プロセスが落ちない。
- wasm のネイティブのテストで、同じ入力の `Module::open` が失敗し、エラーの文を返す。

### ページの木

- 深さ 64 の木（`Pages` を 63 段つなぎ、最後に `Page`）は通り、ページが一枚になる。深さ 65 なら、文のとおりのエラーになる。
- 10 万段の鎖の PDF で、`shem render` が終了コード 1 で終わる。

### CFF

- 局所のサブルーチン j が j + 1 を 4 回呼ぶ 10 本のサブルーチン（4^9 回の呼び出し）を、`charstring_outline` がエラーにする。今のコードは成功を返す。
- 解釈の段数がちょうど 65,536 の charstring は通り、65,537 なら失敗する。
- fixture のすべてのグリフは通る（既存の fixture のテストとゴールデンで確かめる）。

### TrueType

- 8 段の複合グリフが各段で次のグリフを 2 個参照するフォント（読むグリフは 511 個）を、`outline` がエラーにする。256 個ちょうどを読むグリフは通る。

### Flate

- 上限ちょうどの大きさに伸長するデータは通り、1 バイト超えるとエラーになる（上限を小さくして呼ぶ単体のテスト）。
- 2^26 + 1 バイトの 0 を圧縮した内容のストリームを持つ PDF は、文のとおりのエラーになる。圧縮には shem の `png::zlib` を使う。
- 確保の最大のバイト数を数えるテストのバイナリを、`tests/allocations.rs` と同じく自前の allocator で足す（`tests/allocations.rs` は確保の回数を数え、こちらはその時点で確保しているバイト数の最大を数える）。上の PDF の `Document::pages` の最大が 2^27 バイトを超えないことを確かめる。
- `IDAT` が `IHDR` の大きさより長く伸長する PNG は、今と同じ文のエラーになり、同じバイナリで、確保の最大が `IHDR` の大きさの 2 倍を超えないことを確かめる。
- 既存の Flate のテスト（参照の実装との比較を含む）は、上限を 2^26 として、そのまま通る。

### 差分の画像

- 17000 × 1 と 1 × 17000 の PNG の `shem diff` が、終了コード 2 と文のとおりのエラーで、1 秒以内に終わる。差分の画像を書かない。
- 16384 × 16384 ちょうど（2^28 画素）になる組は、上限の判定を通る（画素を作らない単体のテストで、判定の関数だけを確かめる）。

### wasm

- ネイティブの `Module::alloc(usize::MAX)` が 0 を返し、エラーの文が `cannot allocate ...` になる。
- `wasm/test.ts` に、`shem_alloc(0xffffffff)` が 0 を返し、トラップしないことを足す。

### 出力

- 実装の後、fixture の五つの PDF を 72、150、300 dpi の PPM と PNG で描き、main と同じバイト列であることを確かめる。kanikosen の 300 dpi の PPM の `--jobs 1` の時間が、main と変わらないことも確かめる。

## トレードオフ

- 上限を超える入力は、正当な PDF でもエラーになる。上限は fixture の最大の 10 倍以上にしたが、汎用の PDF には、入れ子の深い構造化の木や、大きなフォントを持つものもある。v1 で範囲を広げるときに、上限を見なおす。
- ストリームの上限は、ストリームの種類によらず一つである。フォントのプログラムは、ページの内容より大きくなりやすい。部分集合にしない CJK のフォントは数十 MB になりうるが、Golem はフォントを部分集合にして埋め込む。
- CFF の段数の上限は、数と演算子を区別しない。解釈の時間は段数にほぼ比例するので、時間の上限として素直である。
- 上限の確かめは、どれも整数の比較と足し算だけで、検査と描画の時間はほとんど変わらない見込みである。実装の後に測って確かめる。

## 参考文献

- [docs/audit/2026-11-08-vulnerability-assessment.md](../audit/2026-11-08-vulnerability-assessment.md)：この ADR が扱う所見と、その再現
- Adobe Technical Note #5177, The Type 2 Charstring Format, Appendix B（実装の限界。引数のスタック 48、サブルーチンの入れ子 10） https://adobe-type-tools.github.io/font-tech-notes/pdfs/5177.Type2.pdf
- RFC 1951 DEFLATE Compressed Data Format Specification version 1.3 https://www.rfc-editor.org/rfc/rfc1951
