---
title: Ethornell / BGI
description: Ethornell / BGI のフォーマット、ネイティブ動作、制約。
---

# Ethornell / BGI

`plugin_id: ethornell-rs` · エンジンファミリー: Ethornell / BGI

上流：[xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs)。適応 fork：[Project-Shinkirou/ethornell-rs の shinki ブランチ](https://github.com/Project-Shinkirou/ethornell-rs/tree/shinki)。

## フォーマット

BGI のスクリプトはデコード後のバイト列で判別し、拡張子だけでは判断できません。アーカイブのエントリーは外側が `DSC FORMAT 1.00` で包まれている場合があります。BCS は `BurikoCompiledScriptVer1.00` で始まり、BP のシステムプログラムには `._bp` がよく使われます。ヘッダーのないシナリオは別のバリアントです。BCS の body 相対アドレスと BP のファイル/コードオフセットは別の座標系なので、混同しないでください。

## ネイティブ動作と制約

VM の dispatch、グラフィックス、テキスト、音声、セーブ状態は fork が担当し、ネイティブのアダプターが最終的な CPU フレームを公開します。system、graph、sound、user の各 dispatch グループは、ネイティブの引数スタックの意味をそのまま保ちます。CP932 やローカライズ版のエンコーディングは実際のリリースに合わせて設定する必要があり、一律の変換を推測で適用することはできません。

アーカイブやスクリプトのヘッダーを認識できても、すべての dispatch や初期のバリアントが実装済みとは限りません。エラーレポートにはコマンド番号、位置、再現手順だけを記載し、市販作品の文字列はダンプしないでください。配布時は、fork に含まれる MPL-2.0 の全文と変更の帰属表示を保持します。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
