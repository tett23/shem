# shem ロードマップ

この文書は、shem の版ごとの計画と、まだ決めていないことを置く。
今の振る舞いは [specifications.md](specifications.md) に書き、この文書には書かない。
各項目の仕様は、実装の前に ADR で決める。

## v0.1：今の Golem の出力に合わせる

v0.1 は、今の Golem の Writer が書く PDF の部分集合に合っていればよい。
目的は、Golem の rc4 の見た目の回帰テストに間に合わせることである。
部分集合は、Golem の origin/main の 1254092a（https://github.com/tett23/golem/tree/1254092a2ed5a7fd29bb6f1affb53806cf76c694 ）の出力を調べて決める。

- ファイルの構造：古典的な xref の表、`FlateDecode` のストリーム、ページの木
- ページの内容の演算子：`BT ET Tf Tz Tm Tj Tr w re f m c h q Q cm Do k K cs scn BDC EMC`
- 文字：CFF（`CIDFontType0C`）と TrueType（`FontFile2`）の CIDFont、`Identity-H` と `Identity-V`、縦組みの位置ベクトル、合成の斜体と太字
- 色：DeviceGray、DeviceCMYK、トンボの Separation
- 画像と図版（Form XObject）：中身を描かず、バツ印の付いた灰色の箱で描く
- 出力：PNG と PPM、解像度の指定
- 配布：Linux の x86-64 と aarch64（musl）、macOS の aarch64

範囲の外の入力は、エラーで止める。
ほかの生成系が書く PDF は、多くの場合エラーになる。

## v0.x：Golem に追従し、v1 に向けて広げる

- Golem の Writer が新しい演算子や資源を書くようになったら、先に shem を直してリリースする。
- 画像の復号（JPEG、PNG の標本、`SMask`）と、図版の描画（SVG の図版の Form、取り込んだ PDF の図版の Form）を入れる。Golem の v2 では図版をフレームに合わせて拡大縮小するので、Golem の v1.x か v2 に間に合わせる。
- v1 に向けた範囲の拡大を、少しずつ入れる。

## v1：汎用の PDF のラスタライザ

v1 では、Golem 以外の生成系が書く PDF も描く、汎用の PDF のラスタライザを目指す。
決定性（どの機械でも同じバイト列）、依存を増やさない方針、範囲の外の入力でエラーで止まる原則は、v1 でも保つ。

v1 の範囲は、まだ決めていない。
候補は次のとおりである。

- ファイルの構造：xref のストリーム、オブジェクトのストリーム、増分更新
- フィルタ：`LZWDecode`、`RunLengthDecode`、`ASCIIHexDecode`、`ASCII85Decode`、予測子
- 内容のストリーム：すべての演算子（切り抜き、破線、線端と角、インライン画像、Type 3 のフォントの `d0` と `d1` を含む）
- 文字：単純なフォント（Type 1、TrueType、Type 3）、埋め込みの CMap
- 色：DeviceRGB、ICCBased、CalGray、CalRGB、Lab、Indexed、DeviceN、パターン、シェーディング
- 透明：透明のグループ、ソフトマスク、ブレンドのモード、`ExtGState`
- 画像：`DCTDecode`、`CCITTFaxDecode`、`JBIG2Decode`、`JPXDecode` の復号

## 目視検査の項目

自動のテストでは確かめられない項目を置く。
shem の描いた結果が正しいことは、shem 自身のテストでは示せない（Writer と shem の読み違いが一致しうる）ため、外のビューアーと目で比べる。
リリースの前に、メンテナーが次の項目を確かめる。

- **文字**：Golem の fixture（和文の縦組みと横組み、縦組みの中の欧文、合成の太字と斜体）を、shem と pdftoppm で描いて並べる。字の抜け、字の取り違え、位置のずれ、回転の向きの誤りがないこと。
- **画像と図版の箱**：Golem の `figures` の fixture で、箱の位置と大きさが、pdftoppm が描く画像と図版の位置と大きさに合うこと。
- **色とトンボ**：CMYK の塗りとトンボが、pdftoppm と同じ場所に出ること。色の値は CMYK から RGB への式が違うので一致しなくてよい。
- **差分の画像**：Golem の fixture の一字を変えた PDF を作って前と後を描き、`shem diff` のディレクトリの比較の差分の画像で、変わった字の場所がすぐにわかること（[ADR 0010](adr/0010-diff-images.md)）。

## 未決事項

- v1 の範囲の線引き（上の候補のどれを v1 に入れ、どれを外すか）。
- 暗号化した PDF を v1 で読むか。
- SVG の図版と PDF の図版を区別するか。区別するなら、Golem の出力に印を足すか、別の経路で区別を渡す必要があり、Golem の側と合わせて決める。
- アウトラインの読み取りを、メンテナーが別に作る HarfBuzz と FreeType の統合ライブラリに置き換えるか。
- v1 で crates.io に公開するか。
