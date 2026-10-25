---
title: CMVS
description: CMVS のフォーマット、ネイティブ動作、制約。
---

# CMVS

`plugin_id: shinki.emu.cmvs` · エンジンファミリー: CMVS

ShinkiEMU リポジトリ内の自社コアで、独立した外部 fork はありません。実装は [Source/Families/shinki-emu-cmvs](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/Families/shinki-emu-cmvs)、元のコミットは [ORIGINS](../../../ORIGINS.md) を参照してください。

## フォーマット

リポジトリ内の CMVS コアは CPZ5 を読み込み、ネイティブのスケジューラーで PS2A を実行します。`cmvs.profile.json` は `shinki.emu.cmvs.profile.v2` を使い、順序付きのアーカイブの役割と、役割ごとに選ぶばらファイルを明示的に設定する必要があります。既定のエントリーは `start.ps3` で、Script slot 0 は `cmvs.cfg` から初期化されます。スクリプト、テクスチャー、ネイティブの検索ドメインは互いに独立しています。選択したばらのリソースが破損していても、アーカイブ内の同名ファイルにひそかに差し替えることはしません。

## ネイティブ動作と制約

ネイティブの owner が VM、Scene、メディアを管理し、PS2A の戻り値によるスタックのポップと、ネイティブの 693/694 によるセーブ・ロードを保持します。メニューに入るとシナリオのコンテキストを一時停止します。復帰時はスロットのオブジェクト、テクスチャーの関係、配置を保持し、メニューの一時的なスケジューラー状態をシナリオとして扱うことはしません。破損したセーブの候補が、実行中の状態を部分的に置き換えたり、元のファイルを上書きしたりすることはありません。

コア独自の音声処理が BGM、クロスフェード、効果音をミキシングし、実際のデバイスの形式に変換します。MGV は slot-2 の音声クロックと既存の Theora の規則を保持します。キャンセルするとキューを起こし、終了時は VM、音声、デコーダーの終了を待ちます。CPU リースは終了後も返却できます。

同時に開ける CMVS のセッションは 1 つだけです。現在のリーダーは、未対応の CPZ バージョン、範囲外のパス、未知のコマンド、まだ復元していない文字列ドメインを拒否します。フォントはネイティブのフォントと完全に一致する必要があり、汎用フォントで代用しません。Documents/AppData のセーブルートの動作はまだ復元されていません。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
