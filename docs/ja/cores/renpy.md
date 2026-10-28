---
title: Ren’Py
description: Ren’Py のフォーマット、ネイティブ動作、制約。
---

# Ren’Py

`plugin_id: renpy` · エンジンファミリー: Ren’Py

上流：[renpy/renpy](https://github.com/renpy/renpy)。適応 fork：[Project-Shinkirou/renpy の shinki ブランチ](https://github.com/Project-Shinkirou/renpy/tree/shinki)。

## フォーマットと実行

このコアは公式の Ren’Py ランタイムを組み込んでおり、スクリプトを新しい VM 向けに変換することはしません。スクリプトの実行、Screen Language、ATL、GL2 の合成、メディア、セーブは公式の動作をそのまま使います。固定バージョンの公式 SDK、CPython 3.12.7、Cython 3.2.4、SDL 2.0.20 を組み込み、DLL のチェックサムと出所は `shinki/sdk.json` に記録されています。

## ストレージ

このアダプターは Ren’Py 8.5.3 のセーブ場所の動作を維持します。`config.save_directory` が設定されている場合、ランタイムはゲーム内の `<game>/game/saves` と `%APPDATA%/RenPy/<config.save_directory>` を同期します。未設定の場合は `<game>/game/saves` を使います。セーブトークンとバックアップも上流の解決規則に従い、Shinki 専用のデータディレクトリは使用しません。

## ネイティブ動作と制約

ランタイムのブリッジは常駐するインタープリターの中に残ります。ネイティブのビルドでは display 拡張と実際の共有コアが生成され、パッケージには対応する SDK の DLL と `Resources/` も必要です。終了時やゲームをまたいで開き直すときもインタープリターとネイティブの状態を片付ける必要があるため、コアの DLL だけをコピーしても動きません。

現在の組み込みは Windows x64/MSVC を必要とします。上流の Ren’Py があるプラットフォームに対応していても、このアダプターがそのプラットフォームに移植済みとは限りません。公式の標準アーカイブリーダーはそのまま使い、リリース固有のオフライン変換はレビュー済みのレシピで処理します。Steam、Cubism、Bink、nvdrs、再配布可能な D3DCompiler の DLL は同梱しません。

ランタイムには複数のライセンスが含まれ、そのうち LGPL の依存関係には対応ソースの提供と再リンクの義務があります。公式の表記をすべて保持し、ランタイム全体を MIT と表示してはいけません。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
