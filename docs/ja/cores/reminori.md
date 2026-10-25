---
title: ReMinori
description: ReMinori のフォーマット、ネイティブ動作、制約。
---

# ReMinori

`plugin_id: ReMinori` · エンジンファミリー: Musica

上流：[luoyily/ReMinori](https://github.com/luoyily/ReMinori)。適応 fork：[Project-Shinkirou/ReMinori の shinki ブランチ](https://github.com/Project-Shinkirou/ReMinori/tree/shinki)。

## フォーマット

ReMinori は独立した Musica の実装です。ネイティブの PAZ リーダーは、バージョン 0/1/2、役割ごとに設定された鍵、変換、zlib エントリーを扱います。プライベートな `shinki.emu.musica.keys.v2` の profile でアーカイブのパラメーターを指定でき、指定がない場合は上流の市販 CD-ROM 版のパラメーターを使います。スクリプトでは CP932/GBK としての妥当性を厳密に比較し、それでも残る不正なバイトは明示的に拒否します。

## ネイティブ動作と制約

アダプターは元の VM/IR、GPU 合成、CPAL ミキサー、Media Foundation の動画、メニュー、セーブの実装を保持します。ネイティブの 1024×640、top-down の BGRA8 で CPU リースを渡し、描画サイズの調整は提供しません。再利用される 3 つの CPU スロットが貸し出し中のフレームを保護するため、ネイティブの実行はそのまま続けられます。

`save_directory` はゲームのディレクトリツリーの外にある絶対パスでなければなりません。CP932 だけのセーブはネイティブの形式を保ちます。GBK のスクリプトを使う場合は、既存の UTF-8 port 用コンテナに本文とコメントを保存します。デバイスが失敗しても、診断用の NullAudio にひそかに切り替わることはありません。ネイティブのモーダル要求には、利用可能な型付きのホストサービスが必要です。

eden 向けの実装なので、minori のすべての作品やリリースに当てはまるわけではありません。[リポジトリ内の Musica コア](shinki-emu-musica.md)の実行部を置き換えるものではなく、両者の設定とセーブの識別はそれぞれ独立しています。ReMinori は上流の AGPL-3.0 の義務を引き継ぎます。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
