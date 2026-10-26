# shem

shem は、Golem（https://github.com/tett23/golem ）が書く PDF の部分集合を、どの機械でも同じバイト列の PNG に描くラスタライザである。
実装言語は Rust、ライセンスは MIT とする。

## ディレクトリ構成

- **docs/specifications.md** 今の振る舞いを定める仕様書。日本語で書く。ADR を参照するときはリンクを付ける。
- **docs/adr/** ADR を置く。ファイル名は `0001-feature-details.md` のように連番の prefix と kebab-case の名前を付ける。内容はグローバルの CLAUDE.md の構成（ステータス、文脈、実装すること、実装しないこと、テスト設計、トレードオフ、参考文献）に従う。日本語で書く。
  - コミット済みの ADR はステータスの行とリンクを除き変更不可（adr-guard が強制する）。変更が必要な場合は新たな ADR を作成する。
  - ADR は行頭が `ステータス:` のステータスの行をちょうど一つ持つ。
  - ADR からソースコードを参照するときは、コミットのハッシュを含む GitHub の URL で書く。Golem のコードを参照するときも同じ。
- **docs/audit/**、**docs/benchmarks/** 監査と性能計測の記録。コミット済みのものは変更、削除、移動不可。
- **src/** 実装。

## 文書の言語

- README.md と CONTRIBUTING.md は英語で書き、英語版を正とする。
- README.ja.md と CONTRIBUTING.ja.md は英語版の機械翻訳である。英語版を変えるときは、同じコミットで日本語版も訳し直す。日本語版だけを直すのは、訳の誤りを直すときに限る。
- ADR と docs/specifications.md は日本語で書く。コミットメッセージ、リリースノート、CHANGELOG は英語で書く。

## 実装の約束

- 入力から画素までの経路で浮動小数点数を使わない。整数と固定小数点の演算だけを使う（x86-64 と aarch64、Linux と macOS で同じバイト列を出すため）。
- 描く範囲は Golem の Writer が書く PDF の部分集合に限る。範囲の外のものに出会ったときの振る舞いは ADR で決める。
- 依存は増やさない。必要な処理は、できるかぎり自前で書き、テストを厚く書く。

## skills

- 日本語の文章を書くときは cognitive-rhythm-writing と japanese-tech-writing のスキルを使用する。
