---
title: クイックスタート
description: 3 分で ShinkiEMU Manager を導入し、ゲームとコアを準備して起動する手順。
---

# クイックスタート

ShinkiEMU で最初のビジュアルノベルを起動するまでの 3 分ガイドです。

> [!IMPORTANT]
> **無料・オープンソース宣言**：ShinkiEMU は完全無料のオープンソースソフトウェアです。[公式ポータル](https://project-shinkirou.github.io/ja/shinkiemu/#download)または [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) からのみ入手してください。有料で購入された場合は返金を求め、転売者を通報してください。

## 1. プラットフォーム向け Manager をダウンロード

[ダウンロードページ](https://project-shinkirou.github.io/ja/shinkiemu/#download)または [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) から、プラットフォームに合う配布ファイルを選びます。

| プラットフォーム | パッケージ | コア方式 |
|---|---|---|
| **Windows x64** | ZIP を書き込み可能なフォルダに展開し `shinki-emu-manager.exe` を起動 | アプリ内から導入 |
| **Linux x64** | [ランタイム](troubleshooting.md#linux-デスクトップランタイム)導入後、展開して `./shinki-emu-manager` を起動 | アプリ内から導入 |
| **macOS（Apple Silicon / Intel）** | ZIP を展開し `ShinkiEMU.app` を開く | アプリ内から導入 |
| **Android** | ABI に合うリリース署名済み APK をインストール（汎用 APK は `arm64-v8a` と `x86_64` の両方に対応） | リリースに応じて同梱、または検証済みカタログから導入 |
| **iOS** | 実機／シミュレーター向けのテストパッケージを選択（実機には適切なプロビジョニングが必要） | 同梱 |
| **OpenHarmony** | 実験用 SDK テスト署名 HAP（実機起動未検証） | Musica 同梱 |

デスクトップ版を展開する際は、同梱されている依存ファイル・リソース・ライセンスをそのまま保持してください。

以前の配布内容は[リリース履歴](release-runbook.md#リリース履歴)をご覧ください。現行ソースの Android ビルドには Android 9（API 28）以降が必要です。

## 2. 初回起動時の確認

初回起動時に無料オープンソース・転売防止の案内が表示されます。内容を確認しタイマー完了後にチェックを入れて進みます（後から**設定 → バージョン情報**でも再確認できます）。

- UI 言語は OS に自動追従し、**設定 → 外観 → 言語**から日本語・英語・簡体字中国語へ即時切替可能です。
- コア取得・メタデータ同期・ストアダウンロードを使う場合は**設定 → ネットワーク**で通信を許可してください。

## 3. ゲーム追加とコア導入

正当に入手した作品を次のいずれかで登録します：

1. **ローカルフォルダ**：ライブラリ上部の「フォルダーを追加」から元のゲームフォルダーを選びます。
2. **ストアダウンロード**：通信を許可してライブラリ上部の「ダウンロード」を開き、Steam や DLsite にログインして購入済みの PC 作品を取得します。

デスクトップとコアカタログ対応の Android 版では、**コア**画面から同梱されていないコアを導入できます。iOS と OpenHarmony は同梱コアを使います。

## 4. 起動してプレイ

**ライブラリ**で作品を選び、検出されたコアや設定を確認して**起動**をクリックします。

- 作品ごとの動作実績は[互換性一覧](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を確認してください。
- ストア連携・Luau パッチ・セーブ仕様の詳細は[ユーザーガイド](user-guide.md)へ。
- うまく動かないときは[トラブルシューティング](troubleshooting.md)や [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions) をご活用ください。
