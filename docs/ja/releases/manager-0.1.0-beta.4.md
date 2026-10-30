---
title: Manager 0.1.0-beta.4
description: UI 言語の切り替え、Android の内蔵コア、ネイティブ配布パッケージ。
---

# Manager 0.1.0-beta.4

Manager に英語、簡体字中国語、日本語の UI 翻訳を追加しました。選択した言語は保存され、次回起動時にも適用されます。

Android Manager では Artemis、Siglus、RFVP を同時に内蔵できます。Artemis は Luau に移行し、内蔵コアは Manager とグラフィックスおよび VM の依存関係を共有します。Android の配布ファイルは直接インストールできる APK になりました。各プラットフォームのパッケージに ShinkiEMU のアプリアイコンを含め、ファイル名の `adhoc` 表記を削除しました。

コアのリリースカタログ PR は、信頼済みの metadata ワークフローによる検証後に自動マージされます。今回リリースするのは Manager のみです。既存の beta.3 ダウンロード用コアは互換性のあるネイティブ C ABI v3 を使用します。Android 内蔵コアの更新は Manager APK に含まれ、個別のコアリリースは作成しません。

HarmonyOS ホストは統合期限までに受け入れ検証を完了していないため、今回のリリースには含めません。
