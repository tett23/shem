# ADR 0024: リリースの出自の証明と、CI とリリースの固定と保護

ステータス: 採択（未実装）

## 文脈

2026-11-08 の脆弱性診断（[docs/audit/2026-11-08-vulnerability-assessment.md](../audit/2026-11-08-vulnerability-assessment.md)）は、CI とリリースについても調べた。
スクリプトへの注入、秘密の値の漏れ、キャッシュの汚染、悪意のあるプルリクエストからリリースの成果物に届く経路は、見つからなかった。
一方で、次の指摘があった。

- **出自の証明がない：** リリースの成果物に attestation も署名もない。README の検証は、同じ Release に置いた `SHA256SUMS` との照合である。Release の資産と `SHA256SUMS` を両方差し替えられる者（owner のアカウントや、`contents: write` を持つジョブを乗っ取った者）に対しては、検証が働かない。リポジトリの immutable releases は無効で（`gh api repos/tett23/shem/immutable-releases` が `enabled: false`）、v0.1.0-rc6 も `isImmutable: false` である。
- **adr-guard のタグ：** CI の `adr` のジョブは `tett23/adr-guard@v0.10.0` をタグで参照する（[.github/workflows/ci.yml:90](https://github.com/tett23/shem/blob/80ace3688854dffae088adb4d61d842e54f03767/.github/workflows/ci.yml#L90)）。ほかの action はコミットの SHA で固定している。タグは付け替えられるので、付け替えた者のコードが `adr` のジョブで動く。リポジトリの設定の `sha_pinning_required` は `false` である。
- **Rust の版：** CI とリリースは、その時点の `stable` を入れる（[.github/workflows/release.yml:29](https://github.com/tett23/shem/blob/80ace3688854dffae088adb4d61d842e54f03767/.github/workflows/release.yml#L29) ほか）。同じタグでも、ビルドの時期によってバイナリが変わりうる。リポジトリに `rust-toolchain.toml` はない。
- **タグの保護：** `v*` のタグに ruleset がなく、付け替えも削除もできる。リリースのワークフローは、タグのコミットが main にあることも、CI を通ったことも確かめない（[.github/workflows/release.yml:12](https://github.com/tett23/shem/blob/80ace3688854dffae088adb4d61d842e54f03767/.github/workflows/release.yml#L12)）。
- **required checks：** main の ruleset（id 25879748）の required status checks に、`wasm` のジョブが入っていない。入っているのは `rust` の 3 つ、`adr`、`translations` の 2 つ、`png` である。
- **そのほか：** CI の Deno は `deno-version: v2.x` で浮動である。benchmark のワークフローの `refs` の入力は、`-` で始まる値を検証しない。README の `sha256sum --ignore-missing -c SHA256SUMS` は、macOS 付属の `sha256sum` では、該当するファイルがなくても終了コード 0 になる（README は macOS では `shasum` を使う手順を示している）。

書き込める者は今は owner だけで、どれもすぐに悪用できるものではない。
それでも、利用者がリリースを確かめる手段が、リリースと同じ場所に置いた値だけなのは、README の検証の手順が守るものを誤解させる。

## 実装すること

### リリースの出自の証明

- リリースのワークフローの `release` のジョブで、`gh release create` の前に、`dist/` のすべての成果物（三つの tar.gz と `shem.wasm`）に `actions/attest-build-provenance` で build provenance の attestation を付ける。版は、Node 24 で動く最新のメジャー版（v4.2.2、`4d101475d8b20a2381f78447822ac1eab6504dd8`、内部で `actions/attest` の `node24` を使う）を、コミットの SHA で固定する。
- `release` のジョブだけに `id-token: write` と `attestations: write` を足す。ほかのジョブの権限は変えない。
- README のインストールの節に、`gh attestation verify <ファイル> -R tett23/shem` で確かめる手順を足す。`SHA256SUMS` との照合は、転送の破損を確かめるもので、Release の書き換えは防がないことを書く。README.ja.md も同じコミットで訳しなおす。
- リポジトリの設定で、immutable releases を有効にする（`gh api -X PUT repos/tett23/shem/immutable-releases`）。有効にした後のリリースは、資産とタグを変えられない。

### action とツールの固定

- `tett23/adr-guard@v0.10.0` を、`tett23/adr-guard@624df60fe8956ef52b3a275198ee938175324496 # v0.10.0` にする。`624df60` は、`v0.10.0` のタグが今指すコミットである（`gh api repos/tett23/adr-guard/git/ref/tags/v0.10.0`）。
- リポジトリの Actions の設定で、`sha_pinning_required` を `true` にする（`gh api -X PUT repos/tett23/shem/actions/permissions` に `enabled: true`、`allowed_actions: all`、`sha_pinning_required: true`）。
- CI の Deno を、`deno-version: v2.9.7`（手元で使っている版）にする。

### Rust の版

- `rust-toolchain.toml` を置き、`channel = "1.99.1"`、`profile = "minimal"`、`components = ["rustfmt", "clippy"]` とする。1.99.1 は、今の stable（2026-10-13）である。
- CI、リリース、benchmark のワークフローの `rustup toolchain install stable ...` と `rustup default stable` を、`rust-toolchain.toml` の版を入れる形にする（`rustup toolchain install` を引数なしで実行し、ターゲットは `rustup target add` で足す）。
- benchmark のワークフローは、比べるすべての ref を、ワークフローを動かしたコミットの `rust-toolchain.toml` の版でビルドする（`RUSTUP_TOOLCHAIN` を渡す）。古い ref には `rust-toolchain.toml` がないので、渡さないとその時点の stable でビルドされ、版の違いが時間の差に混ざる。
- README の wasm のビルドの手順（`rustup run stable ...`）と、CONTRIBUTING.md の「Use the stable Rust toolchain.」を、`rust-toolchain.toml` の版を使う書き方にする（日本語版も訳しなおす）。
- Rust の新しい stable が出たら、`rust-toolchain.toml` を上げるプルリクエストを作る。上げるときは、[ADR 0001](0001-scope-golem-contract-and-determinism.md) の決定性のとおり、fixture の画像のバイト列が変わらないことを CI のゴールデンで確かめる。

### タグとリリースのワークフロー

- `v*` のタグに ruleset を作り、更新（付け替え）と削除を禁じる。bypass できる者は置かない。作成は禁じない。
- リリースのワークフローの `verify` のジョブで、タグのコミットが `origin/main` の祖先であることを確かめる（`git merge-base --is-ancestor "$GITHUB_SHA" origin/main`、`fetch-depth: 0`）。祖先でなければ失敗する。main に入ったコミットは、main の ruleset の required checks を通っている。

### required checks

- main の ruleset の required status checks に、`wasm` を足す。

### benchmark の入力と README の検証

- benchmark のワークフローの `refs` の各値が `-` で始まるか空なら、`::error::` を書いて終了コード 64 で止める。`git rev-parse` には `--end-of-options` を渡す。
- README の `sha256sum --ignore-missing -c SHA256SUMS` を、対象の行だけを取り出してから照合する `grep -F " shem-$tag-$target.tar.gz" SHA256SUMS > check && sha256sum -c check` に変える（macOS は `shasum -a 256 -c check`）。行がなければ `grep` が失敗し、照合に進まない。空の入力を `-c` に渡す形（パイプ）にしないのは、macOS 付属の `sha256sum` が、空の入力でも終了コード 0 を返すためである（手元で確かめた。`shasum -a 256 -c` は 1 を返す）。

### 文書

- 仕様の「配布」の節に、attestation、immutable releases、タグの ruleset、`rust-toolchain.toml` の版を書く。
- 監査の記録は変えない（コミットした後は変更しない）。

## 実装しないこと

- バイナリの再現可能なビルド（第三者がビルドしなおして、Release と同じバイト列を得ること）。Rust のバイナリはビルドのディレクトリのパスを含み、tar.gz は時刻と所有者を含む。`--remap-path-prefix` と tar の正規化で近づけられうるが、三つのターゲットで同じバイト列になることを確かめていない。出自は attestation で示す。
- GPG や minisign の署名。鍵の管理が要り、attestation で GitHub の OIDC に結びつけた証明のほうが、鍵を持たずに済む。
- macOS のバイナリの署名と公証。
- Dependabot や Renovate で action と Rust の版を自動で上げること。依存がなく、上げるものが少ないので、手で上げる。
- fork からのプルリクエストの承認の設定（`first_time_contributors`）を変えること。秘密の値がなく、トークンは読み取りだけである。
- 既存のリリース（v0.1.0-rc1 から rc6）に attestation を付けなおすこと。

## テスト設計

設定とワークフローの変更は、自動のテストを書けない。実装の後に、次のとおり動かして確かめる。

### ワークフロー

- この ADR を実装したプルリクエストで、CI のすべてのジョブ（`wasm` を含む）が、`rust-toolchain.toml` の版（`rustc --version` が 1.99.1）で通ることを、ジョブのログで確かめる。
- `sha_pinning_required` を有効にした後、CI がすべて通ることを確かめる（タグで参照する action が残っていれば、ジョブが始まらずに失敗する）。
- 次のプレリリース（v0.1.0-rc7）で、次のことを確かめる。
  - リリースのワークフローが通り、Release が immutable になる（`gh release view v0.1.0-rc7 --json isImmutable` が `true`）。
  - 四つの成果物のそれぞれで、`gh attestation verify <ファイル> -R tett23/shem` が成功する。tar.gz を 1 バイト変えたものでは失敗する。
  - Release のノートを `gh release edit --notes-file` で書きなおせる。書きなおせなければ、ノートを作ってから公開する手順に変える ADR を起こす。
- main にないコミットに `v` のタグを付けても、`verify` のジョブで失敗することを、`v0.0.0-test` のような捨てるタグで確かめる。確かめた後、タグを消す（タグの ruleset は削除を禁じるので、確かめるのは ruleset を作る前とする）。

### 設定

- `gh api repos/tett23/shem/immutable-releases` が `enabled: true` を、`gh api repos/tett23/shem/actions/permissions` が `sha_pinning_required: true` を返す。
- `gh api repos/tett23/shem/rulesets` に `v*` のタグの ruleset があり、規則が `update` と `deletion` である。main の ruleset の required status checks に `wasm` がある。
- `v*` のタグの付け替え（`git push --force` で既存のタグを別のコミットへ）が拒まれることを、v0.1.0-rc7 を作った後に、そのタグで確かめる。

### README の手順

- README の Linux の手順を ubuntu-24.04 で、macOS の手順を手元の Mac で、v0.1.0-rc7 に対して実行し、成功することを確かめる。`target` を存在しない名前にすると、照合が失敗する（終了コードが 0 でない）ことも確かめる。

## トレードオフ

- immutable releases を有効にすると、リリースの資産を差し替えて直すことはできなくなる。成果物に誤りがあれば、新しい版を出す。
- タグの ruleset は owner も迂回できないので、誤って打ったタグを消せない。版の番号を一つ飛ばすことになる。
- Rust の版を固定すると、新しい stable を使うには、プルリクエストで上げる手間が要る。その代わり、版を上げる変更と、出力の変化を、一つのプルリクエストで見られる。
- `sha_pinning_required` を有効にすると、action を足すたびに SHA を調べる手間が要る。
- attestation は、成果物がこのリポジトリのワークフローでビルドされたことを示す。ワークフローやソースが悪意をもって変えられた場合は防がない。それは main の ruleset とタグの ruleset で防ぐ。

## 参考文献

- [docs/audit/2026-11-08-vulnerability-assessment.md](../audit/2026-11-08-vulnerability-assessment.md)：CI とリリースの所見
- actions/attest-build-provenance https://github.com/actions/attest-build-provenance
- GitHub Docs, Using artifact attestations to establish provenance for builds https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds
- GitHub Docs, Immutable releases https://docs.github.com/en/code-security/supply-chain-security/understanding-your-software-supply-chain/immutable-releases
- rustup, Overrides（`rust-toolchain.toml`） https://rust-lang.github.io/rustup/overrides.html
