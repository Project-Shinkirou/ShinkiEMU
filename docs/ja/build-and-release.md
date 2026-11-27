---
title: ソースからのビルド
description: Cargo によるローカルビルド、固定されたネイティブ依存関係、対応ビルド行列。
---

# ソースからのビルド

リポジトリのルートから Cargo で Manager と単独コアをビルドします。ビルド行列と CI には `cargo xtask` を使います。パッケージング、公開手順、過去の配布内容は[リリース・メンテナー手順書](release-runbook.md)、フレーム検証やリプレイ、UI 撮影は[ヘッドレス実行](headless.md)をご覧ください。

## ゲームディレクトリ内の保存先

ShinkiEMU がゲームディレクトリ内に書き込むファイルは `<game>/.shinki/` 配下に置きます。Musica のセーブは `musica/saves/`、ダウンロードしたパッチは `patch/`、処理中のファイルは `staging/`、元ファイルとマニフェストは `backup/<id>/`、上書き適用の結果は `patch-result.json` に保存します。パスを検証し、ディレクトリのリンクはたどりません。サードパーティー製コアは上流の保存規約に従います。

## 入力と依存関係

外部コアの固定コミットは [Cargo.toml](../../Cargo.toml) と [Cargo.lock](../../Cargo.lock) から解決します。[ネイティブ依存関係](../../build/dependencies.toml)、[コア定義](../../build/cores.toml)、[ターゲット定義](../../build/targets.toml)も参照してください。

`git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU` でクローンします。既存のクローンでは `git submodule update --init --recursive` でワークスペースの `ThirdParty/cpal` パッチを初期化します。Git と rustup を導入し、[`rust-toolchain.toml`](../../rust-toolchain.toml) が指定する Rust 1.99.1 を使います。対象の Rust ターゲットと、必要なネイティブコンパイラー、CMake、Ninja、SDK を用意してください。

ネイティブビルドスクリプトは `.work/vcpkg` に固定リビジョンの vcpkg を用意し、マニフェストと triplet ごとに `.work/vcpkg-installs/<triplet>-<key>/installed/` にインストールします。ダウンロードとビルド済みパッケージのキャッシュは `.work/vcpkg-downloads/` と `.work/vcpkg-binary-cache/` にあります。固定リビジョンの vcpkg を共有するには `SHINKI_EMU_VCPKG_ROOT` を指定します。ホストのシステムパッケージと SDK は別途導入します。KiriKiri のパーサージェネレーターはビルドホストで動きます。Windows ではビルドスクリプトが固定版の WinFlexBison を取得できます。他のホストでは Bison を導入するか、`BISON_EXECUTABLE` で指定します（KrKr2 は Bison 3.8.2 以上が必要です）。

パッケージング、`ci` の実行、リリースの準備に先立ち、ホストにライセンス収集ツールを導入します。

```sh
cargo install cargo-about --version 0.9.3 --locked --features cli --bin cargo-about
```

### Windows x64

Visual Studio の C++ ツール、CMake、Ninja、NASM を導入します。Cargo とパッケージングは x64 開発者シェル（`vcvars64.bat`、PowerShell なら `Launch-VsDevShell.ps1 -Arch amd64`）で実行します。`VCRUNTIME140.dll` などの MSVC 再頒布ファイルは `%VCToolsRedistDir%\x64` から取得します。

Windows はファイル名が同じ DLL を再利用するため、Impacto と KiriKiri の `fmt.dll` は ABI を揃える必要があります。現在の Impacto の固定リビジョンは [#37](https://github.com/Project-Shinkirou/ShinkiEMU/issues/37) の対策として fmt 11.2.0 を使用します。

### Linux デスクトップ

Ubuntu 24.04 ではデスクトップビルドに必要な開発パッケージを導入します。

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm yasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev libxinerama-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev libgtk-3-dev libwebkit2gtk-4.1-dev unzip
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev
```

RFVP は `pkg-config` で GStreamer と GLib を検出します。コア ZIP にはこれらのライブラリを同梱しません。実行先の Ubuntu には `libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0t64 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav` を導入します。t64 移行前のリリースでは `libglib2.0-0` を使います。

グラフィックスドライバーを備えた X11 または Wayland 環境で、展開した Manager ディレクトリから `./shinki-emu-manager` を実行します。ディストリビューションの glibc、libstdc++、fontconfig、udev、ALSA と、ストアへのサインインに使う GTK 3、WebKitGTK 4.1 が必要です（Ubuntu では `libwebkit2gtk-4.1-0` を入れると GTK 3 も入ります）。Impacto は `libdrm2` も必要です。KiriKiri2 はシステムの OpenGL、X11、OpenMP、Pango、GTK を使います（Ubuntu 24.04 では `libgl1 libglx0 libopengl0 libx11-6 libgomp1 libpangocairo-1.0-0 libpangoft2-1.0-0 libgtk-3-0t64`）。

### Android

ユニバーサル Manager APK には cargo-ndk 4.1.2 と両方の Rust ターゲットが必要です。`ANDROID_HOME` を SDK ディレクトリに、`ANDROID_NDK_HOME` をその中の `ndk/30.0.16248370` に設定します。APK のパッケージングには JDK 27（`JAVA_HOME` とその `bin` を `PATH` に設定）、SDK platform 37.2、build-tools 37.0.0 が必要です。Gradle Wrapper はリポジトリに含まれています。

[Gradle 9.8.2 の互換性表](https://docs.gradle.org/9.8.2/userguide/compatibility.html#java_runtime)では JDK 27 で Wrapper を実行できます。[Android Gradle Plugin 9.4](https://developer.android.com/build/releases/agp-9-4-0-release-notes) の最低要件は JDK 17 です。本プロジェクトは AGP 9.4.1 を使い、Java と Kotlin のバイトコードは引き続き JVM 17 を対象とします。

```sh
cargo install cargo-ndk --locked --version 4.1.2
rustup target add aarch64-linux-android x86_64-linux-android
sdkmanager --sdk_root="$ANDROID_HOME" --channel=0 "platforms;android-37.2" "build-tools;37.0.0" "ndk;30.0.16248370"
```

Impacto は ABI ごとに `cargo xtask package --core impacto --target <triple> --out <新規ディレクトリ>` でビルドします。ターゲットは `aarch64-linux-android` と `x86_64-linux-android` です。Cargo は共有ライブラリを生成し、xtask が対応する実行時ライブラリ、ライセンス、コアの隣に置く `Resources/` をまとめます。Gradle は不要です。

Android 向けの単独 Ren’Py コアをビルドするには、対象 ABI に対応するランタイムが必要です。環境を整えた Ubuntu 24.04 x86_64 ホストでは、Cargo が `.work/native-downloads/renpy-android/<Rust target>/<recipe key>/` に対象ランタイムを用意します。Python 3 と Jinja2、LLVM/Clang/LLD 18、ccache、[CI](../../.github/workflows/ci.yml) に記載の開発パッケージが必要です。Linux 以外では、対象 ABI 用に Linux で用意したランタイムディレクトリ全体（`include/`、`lib/`、`Resources/`、ライセンス表記を含む）を移し、`RENPY_STATIC_ROOT` に指定します。別の ABI をビルドするときは対応するディレクトリに切り替えます。Ren’Py のレシピは固定版の Linux NDK r29 を、ShinkiEMU の Cargo ビルドは上記の NDK r30 を使います。レシピの入力は [Ren’Py](cores/renpy.md) を参照してください。

### Apple

ネイティブビルドには Xcode をインストールし、`xcode-select` で選択してライセンスに同意します。最小バージョンは macOS 13 と iOS 16 です。macOS は `x86_64-apple-darwin` または `aarch64-apple-darwin`、iOS 実機は `aarch64-apple-ios`、シミュレーターは `aarch64-apple-ios-sim` または `x86_64-apple-ios` の Rust ターゲットを導入します。

iOS Manager に組み込む [KrKr2](cores/krkr2.md) には `rustup component add llvm-tools-preview` も必要です。

Linux からビルドする場合は Docker と Buildx、cross 0.2.5、apple-codesign 0.29.0（`rcodesign`）を用意します。macOS 上の [Xcode](https://developer.apple.com/download/all/) から `xcrun --sdk <sdk> --show-sdk-path` で対象 SDK（`macosx`、`iphoneos`、`iphonesimulator`）を探し、非公開で転送します。`SHINKI_APPLE_SDK_DIR` を、展開した `SDKSettings.json`、`System/`、`usr/` を含むディレクトリに設定し、ターゲット用のイメージを作ります。SDK とそれを含むイメージは公開しないでください。

Linux x86_64 用の固定版 [Buildx v0.38.1 バイナリー](https://github.com/docker/buildx/releases/download/v0.38.1/buildx-v0.38.1.linux-amd64) の SHA-256 は `e25d5cd675092bb5d1d03632fdc46cdf3fede7b514dc5f023a105e00d3ce5ad4` です。ダウンロードを確認してから、専用の Docker 設定の `$DOCKER_CONFIG/cli-plugins/docker-buildx` に実行権限を付けて配置できます。sudo は不要です。`docker buildx version` で確認し、イメージの作成に備えて `/` に 8 GiB 以上の空きを確保します。

```sh
cargo xtask apple-image --target aarch64-apple-darwin
cross build --locked --release --target aarch64-apple-darwin -p shinki-emu-manager
```

#### iOS テスト配布

Xcode を導入した macOS では、RFVP、Siglus、Musica、CMVS、Sena、Ethornell、ReMinori、KrKr2 を個別に署名した ABI v3 フレームワークとして同梱します。実機は `aarch64-apple-ios`、シミュレーターは `aarch64-apple-ios-sim` または `x86_64-apple-ios` を選びます。Manager はアプリ内のフレームワークだけをロードし、ダウンロードや静的コアへの切り替えは行いません。

上記の手順で `cargo-about` を導入します。実機向けには `SHINKI_EMU_IOS_SIGN_IDENTITY` に Apple Development の署名 ID を指定し、対応する秘密鍵をキーチェーンに入れます。`SHINKI_EMU_IOS_PROVISIONING_PROFILE` に `org.shinkirou.shinkiemu` とテスト端末に対応する開発用プロファイルを、`SHINKI_EMU_IOS_ENTITLEMENTS` にそのプロファイルに合った権限ファイルを指定します。シミュレーターは ad hoc 署名を使うため、これらの入力は不要です。

```sh
target=aarch64-apple-ios-sim
version=0.1.0-qa.1
cargo xtask package \
  --target "$target" --manager --version "$version" --out ".work/ios-test-$target"
```

シミュレーターの出力は `ShinkiEMU-simulator-test.app` です。実機向けの `*-device-test.ipa` には `Payload/ShinkiEMU.app` とアプリ内の `embedded.mobileprovision` が入ります。各フレームワークを先に署名し、その後にアプリを署名します。CI とリリース準備では、8 個のフレームワークのハッシュと出所を記録した `ios.json` を、対象コミットとアーカイブに照合します。

Manager の起動やスキャンではコアがロードされないことを確認します。各コアで動作実績のあるゲームを開き、ABI v3 と最初のフレームを確認してから、同じプロセスでゲームを開き直します。ログに dyld エラーや Objective-C クラスの重複がないことも確認します。

### OpenHarmony Manager

DevEco Studio と付属の OpenHarmony Native SDK 5.0.5（API 17）を使います。Rust ターゲット `aarch64-unknown-linux-ohos` を導入し、`OHOS_SDK_NATIVE` を `sdk/default/openharmony/native` に設定します。HAP を作るには `SHINKI_EMU_OHOS_SIGNING_DIR` にリポジトリ外のディレクトリを指定します。パッケージャーは SDK のリンカーを設定し、SDK のテスト PKI で署名した HAP を生成します。HAP は実験用で、市販端末へのインストールと実機起動は未確認です。

#### 同梱 Musica の実機テスト

署名済み HAP は Musica を `libs/<abi>/libshinki_emu_core_musica.so` に同梱します。Manager は有効化時に、OS がインストールした自身のライブラリと同じディレクトリ、または同じ HAP 内の固定パスから ABI v3 でロードします。展開済みのリソースや書き込み可能なアプリデータからはコードを読み込みません。[OpenHarmony の musl ローダー](https://github.com/openharmony/third_party_musl/blob/OpenHarmony-5.0.0-Release/ldso/linux/dynlink.c)が HAP から直接読む項目は、非圧縮でページ境界に整列している必要があります。絶対パスを使い、ファイル名だけでの検索を避けます。

パッケージング時に署名、ELF のアーキテクチャとコアのエントリーポイント、HAP 内のライブラリ、リソース、ライセンスのハッシュを検証します。`ohos.json` には署名済み HAP のハッシュと Musica のパス、ハッシュ、ABI、バージョン、ソースコミットを記録します。

API 12 以降の専用 ARM64 端末を用意し、Vulkan、音声出力、開発者デバッグ、SDK テスト PKI への既存の信頼設定を確認します。テスト署名が拒否されたらそこで止め、端末の信頼設定は変更しません。SDK の `hdc` を `PATH` に追加し、PowerShell で HAP を作ります。

```powershell
$version = '0.1.0-qa.1'
cargo xtask package --target aarch64-unknown-linux-ohos --manager --version $version --out .work/ohos-musica-qa
```

パッケージングが成功したらテスト端末を接続し、`hdc list targets` に表示される対象端末のキーを `$device` に設定します。

```powershell
$hap = ".work/ohos-musica-qa/ShinkiEMU-${version}-ohos-arm64-v8a-debug-test-signed.hap"
hdc -t $device install $hap
```

`hdc` がインストールの成功を報告してから、デバッグモードで起動します。

```powershell
hdc -t $device shell aa start -D -a EntryAbility -b org.shinkirou.shinkiemu
```

1. 起動時に DevEco のネイティブデバッガーを接続します。実行を再開する前と、コア一覧を開いて正規に入手した Musica ゲームをスキャンした後に、LLDB の `image list -f` を実行します。どちらにも `libshinki_emu_core_musica.so` が表示されず、`filesDir/ShinkiEMU/diagnostics` の今回のログに `shinki.emu.ohos.core_open` がないことを確認します。
2. 必要ならコア設定を開かずに Musica を選びます。元の PAZ/SC と有効な非公開 `key.toml` を使って再生します。`shinki.emu.ohos.core_open` が一度出た後、インストール先から `shinki.emu.musica` を ABI 3 で開いた `shinki.emu.ohos.core_abi_ready` が出ることを確認します。この時点で `image list -f` にライブラリが表示されます。
3. 起動画面を進め、ゲーム画面が表示されて音楽や音声が聞こえることを確認し、`shinki.emu.ohos.audio_started` と照合します。同じ Manager プロセスでゲームを閉じて再度開き、ライブラリを再ロードせず再生が始まることを確認します。端末、OS ビルド、結果を記録し、鍵とゲームデータは共有しません。

#### x86_64 シミュレーターでのローカル QA

x86_64 の sysroot と `libc++_shared.so` を含む DevEco の OpenHarmony Native SDK API 26 を使います。`OHOS_SDK_NATIVE` とテスト署名用ディレクトリを上記のとおり設定し、`cargo-about` を用意します。

```powershell
rustup target add --toolchain 1.99.1 x86_64-unknown-linux-ohos
cargo xtask package --target x86_64-unknown-linux-ohos --manager --version 0.1.0-qa.1 --out .work/ohos-x64-qa
```

HAP の名前には `ohos-x86_64-local-qa` が付きます。このターゲットの HAP と `ohos.json` はローカル QA 用です。ホスト CI、リリース、公開コアカタログの対象にはなりません。

## ビルド、チェック、テスト

Manager は `shinki-emu-manager`、単独コアは [`Source/Cores/`](../../Source/Cores/) のエントリーパッケージを指定します。パッケージ名はコア ID のピリオドをハイフンに置き換え、`shinki-emu-core-` を先頭に付けたものです。大文字とアンダースコアはそのままです。たとえば `rfvp` は `shinki-emu-core-rfvp`、`shinki.emu.musica` は `shinki-emu-core-shinki-emu-musica` です。

```sh
cargo build --locked --release -p shinki-emu-manager
cargo build --locked --release -p shinki-emu-core-shinki-emu-musica
cargo check --locked -p shinki-emu-core-rfvp
cargo test --locked -p shinki-emu-musica
cargo test --locked -p xtask -p shinki-emu-build
```

特定のネイティブターゲットには `--target <triple>` を指定し、ホスト上のテストは対象の処理を持つクレートに対して実行します。ローカルビルドの構成は Cargo feature で切り替えます。たとえば Manager に `--features no-core-download` を指定できます。Manager の製品バージョンは `SHINKI_EMU_PRODUCT_VERSION` で上書きでき、省略時は Manager のパッケージバージョンを使います。

Android では cargo-ndk で API 28 を指定します。以下は ARM64 Manager と x86_64 の単独 Siglus コアの例です。

```sh
cargo ndk -t arm64-v8a -P 28 build --locked --release -p shinki-emu-manager --lib
cargo ndk -t x86_64 -P 28 build --locked --release -p shinki-emu-core-siglus_rs
```

既定の Manager は別個のコアバイナリを読み込みます。Android の単体 ZIP をインストールするには、検証済みの `android_dynamic` ポリシーが必要です。iOS はアプリ内の署名済み `ios_framework` を、OHOS は署名済み HAP 内の独立した Musica ライブラリを読み込みます。iOS と OHOS ではダウンロードしたコアをインストールできません。対応ターゲットは [`build/cores.toml`](../../build/cores.toml) で確認できます。ビルド対応はゲームの動作確認とは別です。

### コンパイル時に組み込むストア

Manager は `store-steam`、`store-dlsite`、`store-dmm` を既定で有効にします。Steam のみ有効にするには Cargo に `--no-default-features --features store-steam` を指定します。ストアを追加するには `shinki-emu-store` の trait を実装するクレートを用意し、任意の依存関係と feature を追加して `manager_app/store.rs` に登録します。プロバイダーは不透明なプラン、秘密情報を含まない固定エラー、所有済み作品を返します。一時ファイルは指定された `.shinki/store/<provider>/` に置き、認証情報は `SecretStore` に保存します。

## コアの追加と固定リビジョンの更新

1. 既存の近い例に合わせて `Source/Cores/<id>/` にエントリークレートを追加します。リポジトリ内の Rust コアは実装をパス依存で参照し、`cdylib` から ABI の query を公開します。Rust の fork はワークスペース依存を継承し、必要なら単独ビルド用のエクスポート feature を有効にします。ネイティブアダプターはビルドスクリプトからライブラリを渡します。
2. ルートの [`Cargo.toml`](../../Cargo.toml) の `[workspace.dependencies]` に fork の `git` URL と完全な `rev` を指定します。`build/cores.toml` には ID、ライセンス・通知、ターゲットの対応状況、外部コアの上流情報とリリース連番を記載します。`adapter_licenses` は fork のルート、`licenses` は ShinkiEMU のルートからの相対パスです。
3. `--locked` を付けずに `cargo metadata --format-version 1` を一度実行し、[`Cargo.lock`](../../Cargo.lock) を更新します。差分を確認し、意図した追加やリビジョンの変更だけを取り込みます。依存関係を一括更新せず、マニフェストとロックファイルを一緒にコミットします。通常のビルドでは `--locked` を使います。

外部コアの**公開ソースは `Cargo.lock` に記録された公開 fork のコミット**です。パッケージ公開前に、そのコミットを取得できる状態にしてください。リポジトリ内のコアのソースは ShinkiEMU のリリースコミットです。

### ローカルの fork で作業する

ローカルの fork はこのリポジトリの外に置き、ワークスペースの `Cargo.toml` に一時的なパッチを追加します。テーブルのキーは元の Git URL、パスはローカルの fork 内のアダプタークレートです。RFVP の例：

```toml
[patch."https://github.com/Project-Shinkirou/rfvp"]
shinki-emu-rfvp = { path = "../rfvp/crates/shinki-emu-rfvp" }
```

fork の実際のパッケージ名と互換バージョンを使います。`cargo metadata --format-version 1` でパッチを解決し、同じエントリーパッケージをビルドします。リリース用の変更をコミットする前にパッチを外し、公開 Git リビジョンを使うようロックファイルを更新します。

## マトリクスと CI

```sh
cargo xtask matrix
cargo xtask matrix --json --ci --since origin/main
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

`matrix` は宣言された組み合わせと対象外の理由を表示します。JSON には対応するホストランナーの行が含まれます。`--ci` は ARM64 行でユニバーサル APK を作る場合、重複する Android x86_64 の Manager 行を除きます。`--since` は変更の影響を受ける行に絞ります。`ci` は選択した行をパッケージングし、成果物か対象外の理由を報告します。OpenHarmony はローカルの DevEco で処理します。

## アプリアイコン

Windows、macOS、iOS、Android、Linux の Manager パッケージは `Assets/Branding/shinkiemu-icon.svg` を使用します。Android のアダプティブアイコンは前景を分け、OpenHarmony は専用のアイコンリソースを使用します（[ブランドガイドライン](../../Assets/Branding/BRANDING.md)）。

## パッケージングとリリース

パッケージコマンド、署名、外部コアのバージョン、公開条件は[パッケージングとリリース](release-runbook.md#パッケージングとリリース)を参照してください。
