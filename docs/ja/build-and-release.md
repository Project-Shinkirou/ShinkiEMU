---
title: ビルドとリリース
description: Cargo によるオーケストレーション、コアのビルドスクリプト、パッケージング、明示的な blocked 状態。
---

# ビルドとリリース

リポジトリのルートで Cargo を使い、Manager や単独コアをビルドします。ターゲット一覧、パッケージング、リリース、メタデータには `cargo xtask` を使います。入力リプレイやフレーム・UI の確認は[ヘッドレス実行](headless.md)を参照してください。

## ゲームディレクトリ内の保存先

ShinkiEMU がゲームディレクトリ内に書き込むファイルは `<game>/.shinki/` 配下に置きます。Musica のセーブは `musica/saves/`、ダウンロードしたパッチは `patch/`、処理中のファイルは `staging/`、元ファイルとマニフェストは `backup/<id>/`、上書き適用の結果は `patch-result.json` に保存します。パスを検証し、ディレクトリのリンクはたどりません。サードパーティー製コアは上流の保存規約に従います。

## 入力と依存関係

`git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU` でクローンします。既存のクローンでは `git submodule update --init --recursive` でワークスペースの `ThirdParty/cpal` パッチを初期化します。Git と rustup を導入し、[`rust-toolchain.toml`](../../rust-toolchain.toml) が指定する Rust 1.99.1 を使います。対象の Rust ターゲットと、必要なネイティブコンパイラー、CMake、Ninja、SDK を用意してください。

ネイティブビルドスクリプトは `.work/vcpkg` に固定リビジョンの vcpkg を用意し、マニフェストと triplet ごとに `.work/vcpkg-installs/<triplet>-<key>/installed/` にインストールします。ダウンロードとビルド済みパッケージのキャッシュは `.work/vcpkg-downloads/` と `.work/vcpkg-binary-cache/` にあります。固定リビジョンの vcpkg を共有するには `SHINKI_EMU_VCPKG_ROOT` を指定します。ホストのシステムパッケージと SDK は別途導入します。KiriKiri のパーサージェネレーターはビルドホストで動きます。Windows ではビルドスクリプトが固定版の WinFlexBison を取得できます。他のホストでは Bison を導入するか、`BISON_EXECUTABLE` で指定します（KrKr2 は Bison 3.8.2 以上が必要です）。

パッケージング、`ci` の実行、リリースの準備に先立ち、ホストにライセンス収集ツールを導入します。

```sh
cargo install cargo-about --locked --version 0.9.3
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

Android Manager は Ren’Py を組み込みます。環境を整えた Ubuntu 24.04 x86_64 ホストでは、Cargo が `.work/native-downloads/renpy-android/<Rust target>/<recipe key>/` に対象ランタイムを用意します。Python 3 と Jinja2、LLVM/Clang/LLD 18、ccache、[CI](../../.github/workflows/ci.yml) に記載の開発パッケージが必要です。Linux 以外では、対象 ABI 用に Linux で用意したランタイムディレクトリ全体（`include/`、`lib/`、`Resources/`、ライセンス表記を含む）を移し、`RENPY_STATIC_ROOT` に指定します。別の ABI をビルドするときは対応するディレクトリに切り替えます。Ren’Py のレシピは固定版の Linux NDK r29 を、ShinkiEMU の Cargo ビルドは上記の NDK r30 を使います。レシピの入力は [Ren’Py](cores/renpy.md) を参照してください。

### Apple

ネイティブビルドには Xcode をインストールし、`xcode-select` で選択してライセンスに同意します。最小バージョンは macOS 13 と iOS 16 です。macOS は `x86_64-apple-darwin` または `aarch64-apple-darwin`、iOS 実機は `aarch64-apple-ios`、シミュレーターは `aarch64-apple-ios-sim` または `x86_64-apple-ios` の Rust ターゲットを導入します。

iOS Manager に組み込む [KrKr2](cores/krkr2.md) には `rustup component add llvm-tools-preview` も必要です。

Linux からビルドする場合は Docker と Buildx、cross 0.2.5、apple-codesign 0.29.0（`rcodesign`）を用意します。macOS 上の [Xcode](https://developer.apple.com/download/all/) から `xcrun --sdk <sdk> --show-sdk-path` で対象 SDK（`macosx`、`iphoneos`、`iphonesimulator`）を探し、非公開で転送します。`SHINKI_APPLE_SDK_DIR` を、展開した `SDKSettings.json`、`System/`、`usr/` を含むディレクトリに設定し、ターゲット用のイメージを作ります。SDK とそれを含むイメージは公開しないでください。

Linux x86_64 用の固定版 [Buildx v0.38.1 バイナリー](https://github.com/docker/buildx/releases/download/v0.38.1/buildx-v0.38.1.linux-amd64) の SHA-256 は `e25d5cd675092bb5d1d03632fdc46cdf3fede7b514dc5f023a105e00d3ce5ad4` です。ダウンロードを確認してから、専用の Docker 設定の `$DOCKER_CONFIG/cli-plugins/docker-buildx` に実行権限を付けて配置できます。sudo は不要です。`docker buildx version` で確認し、イメージの作成に備えて `/` に 8 GiB 以上の空きを確保します。

```sh
cargo xtask apple-image --target aarch64-apple-darwin
cross build --locked --release --target aarch64-apple-darwin -p shinki-emu-manager
```

#### iOS の非公開フレームワーク構成

Xcode を導入した macOS で `ios-framework-cores` を有効にすると、Shinki CMVS と Musica の静的登録が外れ、署名付きの ABI v3 フレームワークとして同梱されます。他のコアは静的なままです。`iphoneos` は `aarch64-apple-ios`、`iphonesimulator` は `aarch64-apple-ios-sim` または `x86_64-apple-ios` のうち 1 つを選びます。

上記の手順で `cargo-about` を導入します。実機向けには `SHINKI_EMU_IOS_SIGN_IDENTITY` に Apple Development の署名 ID を指定し、対応する秘密鍵をキーチェーンに入れます。`SHINKI_EMU_IOS_PROVISIONING_PROFILE` に `org.shinkirou.shinkiemu` とテスト端末に対応する開発用プロファイルを、`SHINKI_EMU_IOS_ENTITLEMENTS` にそのプロファイルに合った権限ファイルを指定します。シミュレーターは ad hoc 署名を使うため、これらの入力は不要です。

```sh
target=aarch64-apple-ios-sim
version=0.1.0-qa.1
SHINKI_EMU_MANAGER_FEATURES=ios-framework-cores cargo xtask package \
  --target "$target" --manager --version "$version" --out ".work/ios-framework-qa-$target"
```

シミュレーターの出力は `ShinkiEMU-simulator-test.app` です。実機向けの `*-device-test.ipa` には `Payload/ShinkiEMU.app` とアプリ内の `embedded.mobileprovision` が入ります。アプリ内の配置は `Frameworks/<id>.framework/<id>` と `cores/<id>/core.json` です。フレームワークを先に署名し、その後にアプリを署名します。Manager の起動やゲームのスキャンでは、どちらのフレームワークもロードされないことを確認します。各コアで既知のゲームを起動して ABI v3 と最初のフレームを確認し、同じプロセスで閉じて再び開きます。ログに dyld エラーや Objective-C クラスの重複がないことも確認します。

### OpenHarmony Manager

DevEco Studio と付属の OpenHarmony Native SDK 5.0.5（API 17）を使います。Rust ターゲット `aarch64-unknown-linux-ohos` を導入し、`OHOS_SDK_NATIVE` を `sdk/default/openharmony/native` に設定します。HAP を作るには `SHINKI_EMU_OHOS_SIGNING_DIR` にリポジトリ外のディレクトリを指定します。パッケージャーは SDK のリンカーを設定し、SDK のテスト PKI で署名した HAP を生成します。HAP は実験用で、市販端末へのインストールと実機起動は未確認です。

#### 同梱 Musica の実機テスト

既定の OHOS Manager は Musica を静的に組み込みます。非公開テスト用の `ohos-bundled-musica` を有効にすると、署名済み HAP に独立した `libshinki_emu_core_musica.so` を同梱し、利用開始時にロードします。Manager は OS がインストールした自身のライブラリと同じディレクトリ、または同じ HAP 内の固定パスを使います。展開したリソースや書き込み可能なアプリデータからはコードを読み込みません。[OpenHarmony の musl ローダー](https://github.com/openharmony/third_party_musl/blob/OpenHarmony-5.0.0-Release/ldso/linux/dynlink.c)は呼び出し元の名前空間で動作し、HAP から直接読む項目には非圧縮とページ境界への整列が必要です。インストール済みの絶対パスを渡すため、ファイル名だけで環境変数や継承したパスを検索しません。

API 12 以降の専用 ARM64 端末を用意し、Vulkan、音声出力、開発者デバッグ、SDK テスト PKI への既存の信頼設定を確認します。テスト署名が拒否されたらそこで止め、端末の信頼設定は変更しません。SDK の `hdc` を `PATH` に追加し、PowerShell で HAP を作ります。

```powershell
$version = '0.1.0-qa.1'
$env:SHINKI_EMU_MANAGER_FEATURES = 'ohos-bundled-musica'
cargo xtask package --target aarch64-unknown-linux-ohos --manager --version $version --out .work/ohos-musica-qa
Remove-Item Env:SHINKI_EMU_MANAGER_FEATURES
```

パッケージングが成功したらテスト端末を接続し、`hdc list targets` に表示される対象端末のキーを `$device` に設定します。

```powershell
$hap = ".work/ohos-musica-qa/ShinkiEMU-${version}-ohos-arm64-v8a-bundled-musica-debug-test-signed.hap"
hdc -t $device install $hap
```

`hdc` がインストールの成功を報告してから、デバッグモードで起動します。

```powershell
hdc -t $device shell aa start -D -a EntryAbility -b org.shinkirou.shinkiemu
```

1. 起動時に DevEco のネイティブデバッガーを接続します。実行を再開する前と、コア一覧を開いて正規に入手した Musica ゲームをスキャンした後に、LLDB の `image list -f` を実行します。どちらにも `libshinki_emu_core_musica.so` が表示されず、`filesDir/ShinkiEMU/diagnostics` の今回のログに `shinki.emu.ohos.core_open` がないことを確認します。
2. 必要ならコア設定を開かずに Musica を選びます。元の PAZ/SC と有効な非公開 `key.toml` を使って再生します。`shinki.emu.ohos.core_open` が一度出た後、インストール先から `shinki.emu.musica` を ABI 3 で開いた `shinki.emu.ohos.core_abi_ready` が出ることを確認します。この時点で `image list -f` にライブラリが表示されます。
3. 起動画面を進め、ゲーム画面が表示されて音楽や音声が聞こえることを確認し、`shinki.emu.ohos.audio_started` と照合します。同じ Manager プロセスでゲームを閉じて再度開き、ライブラリを再ロードせず再生が始まることを確認します。端末、OS ビルド、結果を記録し、鍵とゲームデータは共有しません。

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

単独コアの `supported_targets`、Manager の `static_manager_targets`、対象外の理由は [`build/cores.toml`](../../build/cores.toml) で確認できます。ビルド対応はゲームの動作確認とは別です。Android の単独コアは確認済みの `android_dynamic` ポリシーがある場合にインストールできます。iOS は同梱コアを使用し、OHOS は Musica を組み込みます。どちらもコアをダウンロードしてインストールすることはありません。`publish_blocked` はビルド対応とは別の配布制限です。

### コンパイル時に組み込むストア

Manager は `store-steam`、`store-dlsite`、`store-dmm` を既定で有効にします。Steam のみ有効にするには Cargo に `--no-default-features --features store-steam` を指定します。ストアを追加するには `shinki-emu-store` の trait を実装するクレートを用意し、任意の依存関係と feature を追加して `manager_app/store.rs` に登録します。プロバイダーは不透明なプラン、秘密情報を含まない固定エラー、所有済み作品を返します。一時ファイルは指定された `.shinki/store/<provider>/` に置き、認証情報は `SecretStore` に保存します。

## コアの追加と固定リビジョンの更新

1. 単独ライブラリを作る場合は、既存の近い例に合わせて `Source/Cores/<id>/` にエントリークレートを追加します。リポジトリ内の Rust コアは実装をパス依存で参照し、`cdylib` から ABI の query を公開します。Rust の fork はワークスペース依存を継承し、必要なら単独ビルド用のエクスポート feature を有効にします。ネイティブアダプターはビルドスクリプトからライブラリを渡します。Manager に静的に組み込む場合は、実装をターゲット別の静的 feature で直接参照し、`static_cores.rs` に Rust の query を登録します。
2. ルートの [`Cargo.toml`](../../Cargo.toml) の `[workspace.dependencies]` に fork の `git` URL と完全な `rev` を指定します。`build/cores.toml` には ID、ライセンス・通知、ターゲットの対応状況、静的組み込み、外部の上流情報とリリース連番を記載します。`adapter_licenses` は fork のルート、`licenses` は ShinkiEMU のルートからの相対パスです。
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

## パッケージングとリリース

`package` はロック済みのリリースビルドを実行し、Cargo が報告するライブラリ、リソース、ネイティブランタイムを新しい出力ディレクトリに配置します。先に `cargo-about` を導入してください。コアパッケージは ABI v3 の `core.json` とファイルハッシュを保持します。`cargo-about` が生成する Rust のライセンス、コア・ネイティブ依存関係の通知、該当する Manager のオフラインメタデータを同梱します。サードパーティーのライセンスや再リンクの義務も引き続き適用されます。

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/manager-package
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.1 --out .work/android-test
```

通常の Android Manager パッケージは両 ABI を 1 個のユニバーサル APK にまとめます。環境を整えた Linux ホストで作成してください。デバッグ鍵用に `GRADLE_USER_HOME` を設定します。リリース署名用の入力がなければデバッグ署名のテスト APK になります。Linux 以外で対象 ABI の Ren’Py ランタイムを用意した場合など、単一 ABI のデバッグ APK が必要なら `--qa-single-abi` を付けます。Android の単独コアはコアパッケージであり、APK や Gradle 署名は不要です。

リリース APK の作成時に `cargo xtask package` は正式な署名証明書、パッケージ ID `org.shinkirou.shinkiemu`、指定された `versionName` と `versionCode`、`minSdk` 28 を照合します。`release.yml` は `github.run_number` を `SHINKI_EMU_ANDROID_VERSION_CODE` に設定します。公開する APK のコードは前回より大きくしてください。ワークフローの作り直し、改名、別ファイルへの差し替えで連番が 1 に戻ると、インストール済みのアプリが更新できなくなります。既存のワークフローファイルを使い続けてください。

macOS では `.app`、iOS ではシミュレーター用のテスト `.app` または実機用のテスト `.ipa` を作ります。Apple の既定は ad hoc 署名で、実機へのインストールには有効なプロビジョニングが必要です。

`SHINKI_EMU_MANAGER_FEATURES` でパッケージング時に追加する Cargo feature をカンマ区切りで指定できます。iOS の `ios-framework-cores` と HAP の `ohos-bundled-musica` はローカル QA 用で、リリースには使いません。

Manager やリポジトリ内のコアのパッケージングとリリースには、製品の SemVer を `--version` で指定します。外部コアでは省略します。

外部コアの識別子は、元のプロジェクトの `upstream_identity`、`Cargo.lock` から解決したアダプターのリポジトリとコミット、`adapter_branch`、コアごとの `release_sequence` から決まります。上流には実際のタグまたはブランチと完全なコミットを記録します。版の形式は `<tag|branch>-<ref slug>-<digest>` です。ref の先頭 16 バイトの ASCII 英数字を小文字にし、他はハイフンに置き換えます。上流のリポジトリ・ref 種別・ref 名・コミット、アダプターのリポジトリ・ブランチ・コミット、十進数の連番を UTF-8 で並べ、各項目に LF を付けて SHA-256 を求めます。タグは Manager が `manager-v<semver>`、リポジトリ内のコアが `core-<slug>-v<semver>`、外部コアが `core-<slug>-<version>` です。タグの slug はコア ID を小文字にし、ピリオドとアンダースコアをハイフンに置き換えます。外部コアのリビジョンを変えるか新しいリリースを出すときは連番を増やします。カタログは識別子を持つエントリーのうち連番が最大のものを選びます。

```sh
cargo xtask release --component rfvp --dry-run
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

`--dry-run` はビルドせずにバージョン、タグ、ターゲットを表示します。`--target` はビルドして `.work/release/<tag>/<target>/` に配布物と `release.json` を用意します。`--publish` は `GH_TOKEN` または `GITHUB_TOKEN` を使って準備済みの配布物を公開し、コアの場合はカタログ更新の PR を作ります。公開時に配布物のハッシュを確認し、カタログには公開ファイルの SHA-256 とサイズを記録します。Windows/Android のみを公開する場合は計画、準備、公開のすべてに `--scope windows-android` を付けます。既定は `all` です。

[リリースワークフロー](../../.github/workflows/release.yml)がホストランナーの成果物を用意します。手動で実行するときは `components` を指定し、Manager と選択したリポジトリ内のコアには共通の製品 `version` を使います。Windows/Android のみなら `release_scope=windows-android`、公開せず準備するなら `prepare_only=true` を指定します。

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
