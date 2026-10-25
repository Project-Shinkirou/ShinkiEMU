---
title: KiriKiri / Kirikiroid2-web
description: KiriKiri / Kirikiroid2-web のフォーマット、ネイティブ動作、制約。
---

# KiriKiri / Kirikiroid2-web

`plugin_id: kirikiroid2-web` · エンジンファミリー: KiriKiri

上流：[fenghengzhi/kirikiroid2-web](https://github.com/fenghengzhi/kirikiroid2-web)。適応 fork：[Project-Shinkirou/kirikiroid2-web の shinki ブランチ](https://github.com/Project-Shinkirou/kirikiroid2-web/tree/shinki)。

## フォーマット

KiriKiri の storage は、XP3、ばらファイル、ネイティブの KAG/TJS リソースを組み合わせます。保持しているネイティブエンジンは PSB/KBAD のパスも処理します。マウント順はネイティブの起動・マウント規則に従い、ファイル名のアルファベット順で代用することはできません。`TJS2100` の識別子を持つパッチスクリプトはバイトコードで、XP3 でも通常のテキストでもありません。

## ネイティブ動作と制約

C++ のコアは共通 ABI を直接エクスポートし、ネイティブの VM、描画、フォント、メニュー、セーブを保持します。Cargo の query バインディングとビルドスクリプトが、実際のネイティブ共有ターゲットを駆動します。WindowEx オーバーレイの所有権と再入可能なクリーンアップは引き続きネイティブのライフサイクルが管理し、子プロセスや IPC によるゲームホストは使いません。

完全なパッケージには実際のネイティブ依存関係と必要なリソースが必要で、Rust のバインディングライブラリだけを配布することはできません。FFmpeg 3.3.9 は動的リンクされており、対応ソースの提供と再リンクの義務を満たす必要があります。XP3 を読み込めても、サードパーティーのネイティブプラグインやリリース固有のアーカイブフィルターに対応しているとは限りません。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
