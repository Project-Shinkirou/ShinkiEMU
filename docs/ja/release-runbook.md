---
title: リリース・メンテナー手順書
description: Cargo によるパッケージング、署名、公開条件とリリース履歴。
---

# リリース・メンテナー手順書

ローカルビルドとテストは[ソースからのビルド](build-and-release.md)をご覧ください。ここでは Cargo を使ったパッケージング、署名、公開条件、過去の配布内容を説明します。

[ゲームディレクトリ内の保存先](build-and-release.md#ゲームディレクトリ内の保存先)は、ローカルビルドにも配布パッケージにも適用されます。

## パッケージングとリリース

`package` はロック済みのリリースビルドを実行し、Cargo が報告するライブラリ、リソース、ネイティブランタイムを新しい出力ディレクトリに配置します。先に `cargo-about` を導入してください。コアパッケージは ABI v3 の `core.json` とファイルハッシュを保持します。`cargo-about` が生成する Rust のライセンス、コア・ネイティブ依存関係の通知、該当する Manager のオフラインメタデータを同梱します。サードパーティーのライセンスや再リンクの義務も引き続き適用されます。

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-qa.1 --out .work/manager-package
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-qa.1 --out .work/android-test
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-qa.1 --out .work/linux-manager
cargo xtask package --target aarch64-linux-android --core impacto --out .work/impacto-arm64
```

テストビルドには `0.1.0-qa.1` など、ほかのビルドと重複しない SemVer を使用します。Gradle の初回同期後は `SHINKI_EMU_GRADLE_OFFLINE=1` でオフラインのパッケージングができます。

通常の Android Manager パッケージは両 ABI を 1 個のユニバーサル APK にまとめます。デバッグ鍵用に `GRADLE_USER_HOME` を設定します。リリース署名用の入力がなければデバッグ署名のテスト APK になります。APK には KiriKiri の Java ホストが含まれます。動的コアは独立した ZIP で配布し、Gradle 署名は不要です。

デバッグ版とリリース版のどちらでも、公式カタログからコアをインストールするか、登録内容と一致する ZIP を取り込みます。Manager を再起動してインストールを反映します。

KiriKiri の Java、AIDL、JAR はネイティブコアと同じ fork のコミットに固定したホスト用 Cargo ライブラリで生成します。Manager の APK を作る際はこれらを一度だけ用意し、ネイティブコアはビルドしません。コアの ZIP は `package --core <id>` で個別に作成します。
Activity のライフサイクルは同梱のアダプターに通知します。コア固有の JNI はアダプター内に置き、ネイティブコアは有効化時に既存の `AndroidContextV1` からプロセスのコンテキストを受け取ります。

Impacto の切り替えでは、`Cargo.lock` の正確なコミットが組織の fork から取得できることを確認し、連番 4 を先に公開します。`cargo xtask release --component impacto --target <triple>` で Android の両 ABI の ZIP と対応ソースを用意し、公式カタログに両方が載ってから静的組み込みを廃止した Manager を公開します。既存の連番 3 の配布物は変更しません。

リリース APK の作成時に `cargo xtask package` は正式な署名証明書、パッケージ ID `org.shinkirou.shinkiemu`、指定された `versionName` と `versionCode`、`minSdk` 28 を照合します。`release.yml` は `github.run_number` を `SHINKI_EMU_ANDROID_VERSION_CODE` に設定します。公開する APK のコードは前回より大きくしてください。ワークフローの作り直し、改名、別ファイルへの差し替えで連番が 1 に戻ると、インストール済みのアプリが更新できなくなります。既存のワークフローファイルを使い続けてください。

macOS では `.app` を作ります。iOS の配布物はテスト用です。シミュレーター用 `.app` は ad hoc 署名、実機用 `.ipa` は Apple Development の証明書と対応する開発用プロファイルを使用します。CI とリリースの記録は 8 個の署名済みフレームワークと製品コミットを照合します。
通常の CI の実機行はビルドとネイティブ依存関係の検証だけを行い、署名シークレットやインストール可能な IPA は扱いません。

`SHINKI_EMU_MANAGER_FEATURES` でパッケージング時に追加する Cargo feature をカンマ区切りで指定できます。OHOS の既定の ARM64 HAP には独立した Musica ライブラリが入り、OpenHarmony SDK のテスト PKI で署名します。x86_64 HAP は引き続きローカル QA 専用です。

Manager やリポジトリ内のコアのパッケージングとリリースには、製品の SemVer を `--version` で指定します。外部コアでは省略します。

外部コアの識別子は、元のプロジェクトの `upstream_identity`、`Cargo.lock` から解決したアダプターのリポジトリとコミット、`adapter_branch`、コアごとの `release_sequence` から決まります。上流には実際のタグまたはブランチと完全なコミットを記録します。版の形式は `<tag|branch>-<ref slug>-<digest>` です。ref の先頭 16 バイトの ASCII 英数字を小文字にし、他はハイフンに置き換えます。上流のリポジトリ・ref 種別・ref 名・コミット、アダプターのリポジトリ・ブランチ・コミット、十進数の連番を UTF-8 で並べ、各項目に LF を付けて SHA-256 を求めます。タグは Manager が `manager-v<semver>`、リポジトリ内のコアが `core-<slug>-v<semver>`、外部コアが `core-<slug>-<version>` です。タグの slug はコア ID を小文字にし、ピリオドとアンダースコアをハイフンに置き換えます。外部コアのリビジョンを変えるか新しいリリースを出すときは連番を増やします。カタログは識別子を持つエントリーのうち連番が最大のものを選びます。

```sh
cargo xtask release --component rfvp --dry-run
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

`--dry-run` はビルドせずにバージョン、タグ、ターゲットを表示します。`--target` はビルドして `.work/release/<tag>/<target>/` に配布物と `release.json` を用意します。`--publish` は `GH_TOKEN` または `GITHUB_TOKEN` を使って準備済みの配布物を公開し、コアの場合はカタログ更新の PR を作ります。公開時に配布物のハッシュを確認し、カタログには公開ファイルの SHA-256 とサイズを記録します。Windows/Android のみを公開する場合は計画、準備、公開のすべてに `--scope windows-android` を付けます。既定は `all` です。

各リリース記録には公開対象の製品コミットが必要です。別のコミットで準備した場合や記録にコミットがない場合は、パッケージを作り直してください。

[リリースワークフロー](../../.github/workflows/release.yml)は手動でのみ実行され、タグでは起動しません。既定の `prepare_only=true` では選んだコンポーネントの配布物を準備するだけです。公開は `prepare_only=false` または `cargo xtask release --publish` で明示的に行います。公開前に実際の配布物のライセンス、対応ソース、再リンク資料を確認してください。プレイ検証はビルドを妨げません。

準備のみの場合は `source_commit` に対象製品の正確な 40 桁のコミットを指定できます。この入力を使うには `prepare_only=true` が必要です。指定時はワークフローを起動した SHA と製品の SHA を区別してください。

全プラットフォーム向け Manager の公開には、`source_commit` を上書きしていない成功実行を 1 件使います。実行 ID を `RUN_ID` に設定し、2 つのコミット SHA が一致することを確認してからダウンロードします。

```sh
gh run view "$RUN_ID" --json headSha --jq .headSha
git rev-parse HEAD
gh run download "$RUN_ID" --pattern 'release-files-manager@*' --dir .work/release
```

サポート対象のすべてに公開可能なパッケージが必要です。デバッグ署名の Android APK は正式版の代わりに使えません。

ダウンロードすると成果物ごとにディレクトリが作られます。その中の `<tag>/<target>/` を `.work/release/<tag>/` にまとめます。DevEco、署名ディレクトリ、`cargo-about` を用意し、同じ作業ツリーで HAP を作って公開します。`VERSION` にはその実行の製品バージョンを設定します。

```sh
cargo xtask release --component manager --target aarch64-unknown-linux-ohos --version "$VERSION"
cargo xtask release --component manager --version "$VERSION" --scope all --publish
```

スナップショットとカタログのコマンドは [Metadata](metadata.md)、ネイティブパッケージの形式は[コア ABI](core-abi.md)を参照してください。

## リリース履歴

リリースノート：[beta.4](releases/manager-0.1.0-beta.4.md)、[beta.5](releases/manager-0.1.0-beta.5.md)、[beta.6](releases/manager-0.1.0-beta.6.md)、[beta.8](releases/manager-0.1.0-beta.8.md)。

**過去のリリース：Manager 0.1.0-beta.8。** タグ [`manager-v0.1.0-beta.8`](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.8) はソースコミット [`051a05bceff139e60ef6f973781ffa1f093fb2fc`](https://github.com/Project-Shinkirou/ShinkiEMU/commit/051a05bceff139e60ef6f973781ffa1f093fb2fc) を指します。2026年11月3日 07:04:31 UTC に公開され、公開資産は19個です。内訳は9プラットフォームのパッケージ、9個の対応ソースアーカイブ、OHOS 引き渡し記録です。[同一コミットの CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/runs/41543961859) は全マトリクスで成功し、[OHOS 引き渡し gate](https://github.com/Project-Shinkirou/ShinkiEMU/actions/runs/41576487998) も成功しました。

| プラットフォーム | beta.8 の実際の配布物 |
| --- | --- |
| Windows x86_64、Linux x86_64、macOS aarch64/x86_64 | Manager のデスクトップパッケージ。単独コアは別途インストールします。 |
| Android | aarch64 と x86_64 の2 ABI を含む正式配布用の単一 universal APK。RFVP、Siglus、Artemis、Musica、KiriKiri2、CMVS（本版で追加）、Impacto、Ren'Py の8コアを組み込みます。 |
| iOS | aarch64 device-test、aarch64 simulator-test、x86_64 simulator-test の3テストパッケージ。それぞれ RFVP、Siglus、Musica、Sena、Ethornell、ReMinori の6コアを組み込みます。 |
| OHOS aarch64 | Musica の1コアを組み込む実験的な debug-test 署名 HAP。OpenHarmony SDK テスト PKI を使用します。実機/エミュレーター起動と市販デバイスへのインストールは未検証です。 |

Android の正式配布用証明書 SHA256 は引き続き `c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc` です。リリース資産一覧と当該ソースコミットの [`build/cores.toml`](https://github.com/Project-Shinkirou/ShinkiEMU/blob/051a05bceff139e60ef6f973781ffa1f093fb2fc/build/cores.toml) に配布対象と組み込みコアを記録しています。Android の2 ABI は同じ APK に含まれます。新しい Musica パッチには Musica コア `0.1.0-beta.5` 以降が必要です。**Manager beta.7 およびそれ以前の公開タグと資産は変更しません。**

**過去のリリース：Manager [0.1.0-beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7)。** `fd9c78a4cabf1d80430744e4bb5d140c2a837bb9` から9ターゲットを公開しました。Android は7コア、iOS の実機/シミュレーター用3テストパッケージは6コアを組み込み、OHOS は SDK debug-test-PKI で署名しています。公式 Windows バイナリは内部版を `0.1.0-beta.6` と報告します。後続のソース機能やコア pin の更新によって、これらの過去の配布物は変わりません。

[KiriKiri2 連番2のリリース](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-kirikiroid2-web-branch-web-1e3603089b4951c8e2831d978ea6fe5a8097bceeb0a56fdeb65aa3953e67eecf) は、製品 `fe4e6f2d82c8b61f3f14e13d48af6d57722b9952`、adapter `d72046c28e6ada623acdb1d72f7075edd488797d` から Windows、Linux、両 macOS アーキテクチャを公開しています。同じ adapter は beta.7 Android に静的組み込み済みですが、iOS や OHOS への KiriKiri 追加を意味しません。デスクトップ版には対応ソースと元ライセンスを同梱し、FMOD の条項を別に維持します。
[Impacto 連番3のリリース](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-impacto-tag-0-10-8-930-96de7434c8b1a57df7ff24dcee829282194873fe596677056fa168485cb1555e) は、製品コミット `5b197c1c656186d3a535bb3486311623516166e5`、upstream `0.10.8.930`、adapter `34b89c29123d025d96f73ac9a746fb388e572065` から Windows、Linux、両 macOS アーキテクチャのパッケージを公開しています。4対象の対応ソース・ネイティブ入力・ライセンス・パッケージ監査と、認証なしでの全8バイナリ/ソース添付ファイルの実ダウンロード・ハッシュ照合が完了しました。この単独版は Manager6 内蔵の `910a820ba3ea8d4dd8d6e19c560df1d5a3bc5157`/連番2を置き換えず、公開済み連番1の資産も変更しません。カタログは公開完了の順番ではなく連番3を選びます。
[Ren’Py 連番1のリリース](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-renpy-tag-8-5-3-26051504-5f537560e1a1b852070dec65946d6c027e9f4f7482bddd8349ed47960c383133) は、製品コミット `05326439dcfafb81df79f3dabe0997fb696ad3f3`、upstream `8.5.3.26051504`、adapter `a52d9d9c006223c2b5a276c05e1b97ffeecbe66f` から4デスクトップ対象を公開しています。全対応ソースパッケージで、107バイトのシンボリックリンク先を含む固定再ビルド用レシピ389/389項目の実物確認が完了しました。公開された全8バイナリ/ソース添付ファイルを認証なしで実際に取得し、受け入れ済み記録とハッシュを照合しました。元のネイティブライセンス本文も固定 fork と原入力に対してバイト単位で確認済みです。
