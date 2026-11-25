---
title: ShinkiEMU ドキュメント
description: プレイヤー・コントリビューター・開発者向けの ShinkiEMU ドキュメント総合案内。
---

# ShinkiEMU ドキュメント

ShinkiEMU は、クロスプラットフォーム対応のゲームライブラリマネージャー **Manager** とモジュール式の**ネイティブエンジンコア**で構成され、現代の PC やスマートフォンでビジュアルノベルをネイティブ動作させます。共有ランタイムライブラリは姉妹エンジン [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) から提供されています。

## はじめての方へ（プレイヤー）

- [クイックスタート](quick-start.md)：3 分でダウンロード・ゲーム追加・コア導入・起動まで完了。
- [ユーザーガイド](user-guide.md)：表示言語、更新確認、コア管理、Steam/DLsite ダウンロード、Luau パッチ、原版セーブ互換。
- [トラブルシューティング](troubleshooting.md)：Linux ランタイム、Android GPU 設定、DLsite 診断、よくある問題と解決策。

## コミュニティと参加

- [コントリビュートガイド](contributing.md)：コード不要の貢献方法、構成図、UI 翻訳（`.po`）、AI 活用ガイドラインと PR 手順。
- [メタデータとパッチ投稿](metadata.md)：互換性レポートと Luau パッチの提出方法およびレビュー規則。
- [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions)：質問、情報共有、アイデア相談の場。

## 開発者・メンテナー向け

- [ソースからのビルド](build-and-release.md)：固定ツールチェーン、`cargo xtask` コマンド、プラットフォーム別ビルド入口。
- [コア ABI と SDK](core-abi.md)：C ABI v3 ライフサイクル、機能ビット、非同期テキスト置換、CPU フレームリース仕様。
- [ヘッドレス実行と UI 撮影](headless.md)：`shinki-emu-headless` による描画検証、入力再生、Manager 画面キャプチャ。
- [リリース・メンテナー手順書](release-runbook.md)：依存ピン固定、CI マトリクス、Apple/OHOS 引き渡し、対応ソース・ライセンス収集、リリース記録。

## コアの概念と各コア仕様

ShinkiEMU では、**エンジン系統（family）**が元のエンジン形式を、**コア（core）**がその具体的実装を、**`plugin_id`** が導入やメタデータで用いる大文字・小文字区別の識別子を表します（`shinki.emu.musica` と `ReMinori` は同じ Musica 系統ですが、実行系・設定・セーブは独立しています）。

[RFVP](cores/rfvp.md) · [Siglus](cores/siglus-rs.md) · [Artemis](cores/art3m1s-core.md) · [Musica（独自）](cores/shinki-emu-musica.md) · [ReMinori](cores/reminori.md) · [CMVS（独自）](cores/shinki-emu-cmvs.md) · [KiriKiri2](cores/kirikiroid2-web.md) · [impacto](cores/impacto.md) · [Ren’Py](cores/renpy.md) · [Sena](cores/sena-rs.md) · [Ethornell](cores/ethornell-rs.md)

最新の公開コアは[コアカタログ](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、作品別の実測結果は[互換性一覧](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)をご確認ください。
