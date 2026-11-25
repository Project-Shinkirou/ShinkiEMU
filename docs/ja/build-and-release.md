---
title: ソースからのビルド
description: ローカル開発環境、cargo xtask によるビルド・検証コマンド、プラットフォーム別ビルド入口。
---

# ソースからのビルド

本書は ShinkiEMU をローカルでビルド・テストする開発者向けガイドです。依存バージョン固定、Apple クロスビルド、OpenHarmony 引き渡し、対応ソース／ライセンス収集、リリース履歴などの詳細は[リリース・メンテナー手順書](release-runbook.md)をご覧ください。ヘッドレス検証と UI スクリーンショットは[ヘッドレス実行](headless.md)を参照してください。

## ツールチェーンと前提条件

- **Rust ツールチェーン**：Rust 2024 edition、`rust-toolchain.toml` で **Rust 1.99.1** に固定。
- **サブモジュール付きクローン**：Git サブモジュールは `ThirdParty/cpal` のみです（外部コアは `cargo xtask deps` が `build/cores.toml` から取得します）：
  ```sh
  git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU.git
  ```
- **マニフェスト**：`build/dependencies.toml`（ツール・依存）、`build/cores.toml`（コア一覧）、`build/targets.toml`（ターゲットと runner）。

| プラットフォーム | 主な前提条件 |
|---|---|
| **Windows x64** | x64 MSVC 開発環境、CMake、Ninja、Git |
| **Linux x64** | GCC/Clang、CMake、Ninja、pkg-config、NASM、WebKitGTK 4.1 開発パッケージ、グラフィック／音声開発ライブラリ |
| **macOS / iOS** | ネイティブ Xcode（または Linux の `cross 0.2.5` + 正規 Apple SDK。詳細は[リリース手順書](release-runbook.md)） |
| **Android** | JDK 27、Android SDK platform 37.2、build-tools 37.0.0、NDK r30（`30.0.16248370`）、`cargo-ndk 4.1.2` |
| **OpenHarmony** | DevEco Studio Native SDK 5.0.5（API 17、`OHOS_SDK_NATIVE`） |

## よく使う `cargo xtask` コマンド

リポジトリルートから実行し、変更した対象（`--manager` または単一 `--core`）のみを指定してください：

```sh
cargo xtask matrix
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc --core rfvp

cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
cargo xtask metadata validate
```

- 引数なしの `cargo xtask test` は `xtask` とビルド補助テスト（アイコン生成やメタデータ解析を含む）を実行します。
- 未コミットの外部コア変更を試す場合は `deps --allow-dirty-sources` を使用できます（正式リリースでは拒否されます）。

## ストア機能フラグ

`shinki-emu-store` はオブジェクト安全な provider/session trait とキーストア境界を定義します。標準では `store-steam`、`store-dlsite`、`store-dmm` が有効です。Steam のみ組み込む場合は `--no-default-features --features store-steam` を指定します。

## Linux デスクトップ

Ubuntu 22.04 / 24.04 に開発パッケージをインストールします：

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev libxinerama-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev mesa-vulkan-drivers xvfb xauth unzip jq
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav libgtk-3-dev libwebkit2gtk-4.1-dev
```

```sh
cargo xtask deps --target x86_64-unknown-linux-gnu
export SHINKI_EMU_PRODUCT_VERSION=0.1.0-beta.8
cargo xtask build --target x86_64-unknown-linux-gnu --manager --release
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-beta.8 --out .work/linux-manager
```

エンドユーザー向け最小ランタイムは[トラブルシューティング — Linux デスクトップランタイム](troubleshooting.md#linux-デスクトップランタイム)を参照してください。

## Android ローカルテスト APK

`JAVA_HOME`（JDK 27）、`ANDROID_HOME`、`ANDROID_NDK_HOME`（`ndk/30.0.16248370`）を設定して実行します：

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo xtask deps --target aarch64-linux-android
cargo xtask deps --target x86_64-linux-android
export SHINKI_EMU_PRODUCT_VERSION=0.1.0-beta.8
cargo xtask build --target aarch64-linux-android --manager --release
cargo xtask build --target x86_64-linux-android --manager --release
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.8 --out .work/android-test
```

リリース署名鍵がない場合はテスト用 debug 署名 APK が出力されます。

## ディスク配置とアプリアイコン

- ShinkiEMU がゲームフォルダ内へ書き込むファイルはすべて `<game>/.shinki/` 以下に隔離されます。
- Windows、macOS、iOS、Android、Linux の Manager パッケージは `Assets/Branding/shinkiemu-icon.svg` を使用します。Android のアダプティブアイコンは前景を分け、OpenHarmony は専用のアイコンリソースを使用します（[ブランドガイドライン](../../Assets/Branding/BRANDING.md)）。

## メンテナー向けリリース手順

詳細な依存ピン固定、Apple/OHOS パッケージング、対応ソース・ライセンス収集およびリリース履歴は[リリース・メンテナー手順書](release-runbook.md)をご覧ください。

## パッケージングとリリース

[パッケージングとリリース](release-runbook.md#パッケージングとリリース)を参照してください。
