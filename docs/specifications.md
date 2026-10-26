# shem 仕様書

shem は、PDF のページを PNG の画像に描くラスタライザである。
最初の目的は、Golem（https://github.com/tett23/golem ）の PDF の出力の見た目の回帰テストで、見た目の変化を見つけることである。
描いた結果が正しいことを示すものではない。
v1 では、汎用の PDF のラスタライザを目指す（[roadmap.md](roadmap.md)）。

この文書は、shem の今の振る舞いを定める。
設計の決定は `docs/adr/` に連番の ADR として置く。

## 状態

まだ何も実装していない。
目的と範囲、Golem との取り決め、決定性の約束、描く範囲は、ADR で決めているところである。
版ごとの計画は [roadmap.md](roadmap.md) にある。

## 破棄した ADR

なし。
