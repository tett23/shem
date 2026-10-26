# ADR 0012: ライブラリの利用者に向けた並列の API

ステータス: 採択（未実装）

## 文脈

shem はライブラリ（`shem` のクレート）と、それを使うコマンド（`shem` の実行ファイル）からなる。
[ADR 0009](0009-render-pages-in-parallel.md) では、ページを並列に描く仕組みをコマンドの層に置き、ライブラリの利用者に向けた並列の API を「実装しないこと」に入れた。
[ADR 0010](0010-diff-images.md) の `shem diff` のディレクトリの比較も、同じ仕組みで並列に動く。

今のライブラリが公開する描画の入り口は、一つのページを描く `render_page` と、検査する `check_page` だけである（[src/render.rs:386-395](https://github.com/tett23/shem/blob/702584ad2fa0137d94786b0a6b03c895af86e007/src/render.rs#L386-L395)）。
ライブラリの利用者が複数のページを並列に描くには、次のことを利用者がそれぞれ書くことになる。

- スレッドの起こし方とページの配り方
- エラーの選び方（どのページのエラーを報告するか）
- [ADR 0011](0011-speed-up-without-changing-bytes.md) のフォントのキャッシュを、ページとスレッドの間で共有すること

エラーの選び方を利用者ごとに書くと、報告するエラーがスレッドの終わる順で変わりうる。
すると、決定性の約束（[ADR 0001](0001-scope-golem-contract-and-determinism.md)）が、ライブラリの外で崩れる。
Golem の v3 の GUI では、描画の内容を canvas に渡すことが未決事項になっており、ライブラリとして使われる可能性がある。

そこで、並列の仕組みをライブラリに置いて公開し、コマンドもそれを使う。

## 実装すること

### 並列に仕事を配る関数

`shem::parallel` に、次の関数を置く。

```rust
/// task(0), task(1), … task(count - 1) を、最大 jobs 個のスレッドで実行する。
pub fn run<E, F>(count: usize, jobs: NonZeroUsize, task: F) -> Result<(), (usize, E)>
where
    E: Send,
    F: Fn(usize) -> Result<(), E> + Sync;

/// std::thread::available_parallelism の値。取れなければ 1。
pub fn available_jobs() -> NonZeroUsize;
```

- 番号は、`AtomicUsize` で小さい順に配る。各番号の仕事は、ちょうど一つのスレッドで一度だけ実行する。
- スレッドの数は、`jobs` と `count` の小さいほうとする。1 なら、スレッドを作らずに呼び出したスレッドで順に実行する。
- ある仕事がエラーを返したら、以後は新しい番号を配らない。実行中の仕事は最後まで実行する。
- すべてのスレッドが終わったら、エラーを返した番号のうち最も小さいものと、そのエラーを返す。
- 返すエラーは、スレッドの数と終わる順によらない。最も小さい失敗の番号を `e` とすると、番号は小さい順に配るので、`e` は、ほかのどの失敗の番号よりも先に配られている。配った仕事は最後まで実行するので、`e` の仕事は必ず実行され、そのエラーが集まる。`e` より小さい番号はどれも失敗しない。
- 仕事の中のパニックは、`std::thread::scope` のとおり、呼び出したスレッドに伝わる。

### 文書を描く入り口

`shem::render` に、一つの文書を描くための `Session` を置く。

```rust
pub struct Session<'a> { /* 文書、ページ、解像度、フォントのキャッシュ */ }

impl<'a> Session<'a> {
    pub fn new(doc: &'a Document, dpi: u32) -> Result<Session<'a>>;
    pub fn page_count(&self) -> usize;
    /// 一つのページを検査する（check_page と同じ）。
    pub fn check(&self, page: usize) -> Result<()>;
    /// 一つのページを描く（render_page と同じ）。
    pub fn render(&self, page: usize) -> Result<Canvas>;
    /// pages の各ページを並列に検査し、最も前のページのエラーを返す。
    pub fn check_pages(&self, pages: &[usize], jobs: NonZeroUsize) -> Result<()>;
    /// pages の各ページを並列に描き、描けたページの画像を consume に渡す。
    pub fn render_pages<E, F>(&self, pages: &[usize], jobs: NonZeroUsize, consume: F)
        -> std::result::Result<(), PageError<E>>
    where
        E: Send,
        F: Fn(usize, Canvas) -> std::result::Result<(), E> + Sync;
}

pub enum PageError<E> {
    /// 描画のエラー（Error はページの番号を含む）
    Render(Error),
    /// consume が返したエラーと、そのページの番号
    Consume(usize, E),
}
```

- ページの番号は 0 から数える（`Page::index` と同じ）。範囲の外の番号はエラーにする。
- `Session` は、[ADR 0011](0011-speed-up-without-changing-bytes.md) のフォントのキャッシュを持ち、検査と描画、すべてのページ、すべてのスレッドで共有する。
- `Session` は `Sync`、`Canvas` は `Send` とする。コンパイル時の検査で固定する。
- `check_pages` と `render_pages` は `parallel::run` を使う。報告するのは、`pages` の並びで最も前のページのエラーである。
- `consume` は、ページを描いたスレッドで、ページごとに一度だけ、どの順にも呼ばれうる。`consume` がエラーを返した場合も、描画のエラーと同じく新しいページを配らない。
- 既存の `render_page` と `check_page` は残す。中では一つのページのための `Session` を使う。

### コマンド

- `shem render` と、`shem diff` のディレクトリの比較（[ADR 0010](0010-diff-images.md)）は、`parallel::run` と `Session` を使う。[ADR 0009](0009-render-pages-in-parallel.md) でコマンドの層に置くとした並列の仕組みは、この関数に置き換える。コマンドの振る舞い（`--jobs`、検査の後の書き出し、報告するエラー）は変えない。
- `--jobs` の既定値は `parallel::available_jobs` とする。

### 版と文書

- ライブラリの API の追加で、コマンドの出力は変えないので、パッチバージョンを上げる（[ADR 0001](0001-scope-golem-contract-and-determinism.md)）。v0.x の間、ライブラリの API は版を上げずに変えうることを、README とその翻訳に書く。
- 公開する関数と型に、rustdoc のコメント（並列の約束とエラーの選び方を含む）を書く。仕様書に「ライブラリ」の節を加え、この ADR の約束を書く。

## 実装しないこと

- 非同期の API（`async`）と、外のスレッドプール（rayon など）の受け入れ。
- 描いたページを番号の順に返すイテレーター。順に返すには、先に終わったページの画像を持っておく必要があり、メモリーが `--jobs` 以上に増える。順が要る利用者は、`consume` で受け取ったものを自分で並べる。
- 描画の途中での取り消し（キャンセル）。
- ページの中の並列化（[ADR 0009](0009-render-pages-in-parallel.md) のまま）。
- 複数の文書にまたがるフォントのキャッシュ（[ADR 0011](0011-speed-up-without-changing-bytes.md) のまま）。
- crates.io への公開（[ADR 0001](0001-scope-golem-contract-and-determinism.md) のまま）。

## テスト設計

### `parallel::run`

- `count` が 0、1、100 で、`jobs` が 1、3、16 のすべての組で、各番号の仕事が一度だけ実行される（実行の回数を番号ごとに数える）。
- `jobs` が 1 なら、新しいスレッドを作らず、番号の順に実行する（実行したスレッドの ID と順を記録する）。
- 番号 3 と 7 が失敗し、番号 7 の仕事のほうが先に終わるように番号 3 の仕事を待たせても、返すのは `(3, ..)` である。`jobs` を 1、2、8 にして、それぞれ 50 回繰り返す。
- 失敗の後に新しい番号が配られないこと：番号 0 が失敗する 1000 個の仕事を `jobs` 2 と 8 で実行し、実行された仕事の数が `jobs` + 1 以下であることを確かめる（失敗を知る前に、各スレッドが一つずつ番号を取りうるため）。
- 仕事のパニックが、呼び出したスレッドに伝わる。

### `Session`

- Golem の四つの fixture の全ページについて、`render_pages`（`jobs` 1 と 4）で受け取った画像が、`render` で一ページずつ描いた画像と同じである。
- `render_pages` の `consume` は、ページごとに一度だけ呼ばれる。
- `consume` が 2 ページ目と 3 ページ目でエラーを返すとき、`PageError::Consume(2, ..)`（`pages` の並びで前のもの）が返る。`jobs` 1 と 4 で同じになる。
- 範囲の外の演算子が 2 ページ目と 4 ページ目にある PDF で、`check_pages` が 2 ページ目のエラーを返す。`jobs` 1 と 8 で同じになる。
- 範囲の外のページ番号はエラーになる。
- `Session: Sync` と `Canvas: Send` を、コンパイル時の検査で確かめる。
- 既存の `render_page` と `check_page` のテスト、ゴールデン、コマンドのテスト（[ADR 0009](0009-render-pages-in-parallel.md)、[ADR 0010](0010-diff-images.md) のものを含む）を変えずに通す。

## トレードオフ

- 並列の仕組みをライブラリに置いたので、コマンドとライブラリの利用者が、同じエラーの選び方と同じキャッシュを使える。そのかわり、ライブラリが公開する型と関数が増え、v0.x の間に変えるときの影響が広がる。
- 汎用の `parallel::run` を、番号を配って仕事を呼ぶだけの小さな関数にした。描画にも差分にも使え、決定性の議論もこの関数の中だけで済む。結果を集める部分は呼び出す側の仕事なので、呼び出す側で順をそろえる手間が残る。
- `consume` を描いたスレッドで呼ぶので、画像を集める場所を持たずに済み、メモリーは `jobs` に比例するだけで済む。そのかわり、`consume` はスレッドの間で安全（`Sync`）でなければならず、呼ばれる順も決まらない。
- 失敗の後も実行中の仕事を最後まで実行するので、返すエラーが決まる。その間、ほかのページのファイルが書かれることがある（[ADR 0009](0009-render-pages-in-parallel.md) と同じ）。

## 参考文献

- Rust の `std::thread::scope` https://doc.rust-lang.org/std/thread/fn.scope.html
- Rust の `std::thread::available_parallelism` https://doc.rust-lang.org/std/thread/fn.available_parallelism.html
- Rust の `std::num::NonZeroUsize` https://doc.rust-lang.org/std/num/type.NonZeroUsize.html
- Golem の roadmap の v3 の未決事項（GUI の canvas に渡す描画の内容）https://github.com/tett23/golem/blob/1254092a2ed5a7fd29bb6f1affb53806cf76c694/docs/roadmap.md#L258
