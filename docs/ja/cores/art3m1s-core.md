---
title: Artemis / Art3m1s
description: Artemis / Art3m1s のフォーマット、ネイティブ動作、制約。
---

# Artemis / Art3m1s

`plugin_id: art3m1s-core` · エンジンファミリー: Artemis

上流：[Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)。適応 fork：[Project-Shinkirou/art3m1s-core の shinki ブランチ](https://github.com/Project-Shinkirou/art3m1s-core/tree/shinki)。

## フォーマット

Artemis は `system.ini`、PFS（PF6/PF8）、ネイティブのシナリオ・tag・Lua リソースを読み込みます。標準の数字付きパッチボリューム `.pfs.000` から `.pfs.999` はベースパッケージを上書きし、有効なボリュームの中では番号の大きいものが優先されます。バックアップ用のサフィックスは、パッチとして自動ではマウントされません。ネイティブのばらファイルとパッケージの優先順位はそのまま保たれます。

## ネイティブ動作と制約

コアは Lua 5.1、ネイティブのグラフィックスパス、FFmpeg を保持しています。この Lua の依存グラフは Manager の Luau レシピとは別です。アダプターがスケジューリングとデバイス音声を担当し、長さ上限のある不変の CPU スロットでフレームを渡します。ネイティブのモーダルダイアログは型付きのホストサービスを通り、終了してもセッションが終わるだけで Manager のプロセスは終了しません。

`extra_archive` は相対パスの PFS を明示的に選び、通常の数字付きパッチの連鎖のあとにマウントします。バックアップのスキャンやベースパッケージの書き換えは行いません。descriptor は描画サイズの調整を提供しません。スクリプトの対象やリソースの欠落、未対応のネイティブ UI・メディアコマンドは引き続きエラーになります。タイトル画面を操作できても、ゲーム全体を通して遊べるとは限りません。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
