---
title: RFVP
description: RFVP のフォーマット、ネイティブ動作、制約。
---

# RFVP

`plugin_id: rfvp` · エンジンファミリー: FVP

上流：[xmoezzz/rfvp](https://github.com/xmoezzz/rfvp)。適応 fork：[Project-Shinkirou/rfvp の shinki ブランチ](https://github.com/Project-Shinkirou/rfvp/tree/shinki)。

## フォーマット

FVP は HCB バイトコードを使います。ファイル先頭の little-endian u32 はシステム記述領域のオフセットで、コードは 4 バイト目から始まります。magic 文字列による判別は行いません。記述領域にはエントリーポイント、グローバル変数、syscall テーブルが含まれます。長さ付きの C 文字列は末尾の NUL を長さに含めます。エンコーディングはコアの設定で明示的に選びます。

## ネイティブ動作と制約

fork は元の App、VM、GPU レンダラー、デバイス音声、セーブを保持します。アダプターは不変の CPU リースで最終的なシーンを渡し、Manager は syscall を書き直すことも、ホストの tick で進めることもしません。ネイティブの設定では、元の HCB オフセットで索引付けした UTF-8 の翻訳テーブルや、明示的なフォントと元のメディアクロックを使う movie/BGM の ASS マッピングを選べます。

WMV/ASF/WMA は fork のネイティブの動画・デコーダーのパスで再生します。ネイティブが要求するフォントやゲームのリソースは引き続き実際の入力として必要です。HCB を認識できても、syscall がすべて実装済みであることや、すべてのリリースで動作することを意味するわけではありません。

## セーブパスと形式

元の RFVP エンジンは番号付きセーブを `<game>/save/sNNN.bin`、HCB のグローバルデータを `<game>/save/save.bin` に保存します（`crates/rfvp/src/script/global.rs:134-136` と `setsumei/` のリバースエンジニアリング記録）。fork の `SaveItem` は元のヘッダーの後に独自のランタイム状態ブロックを追加し、グローバルファイルは `GlobalSaveDataV1` を `RFVG` で包むため、どちらも元形式のバイト単位の実装ではありません。fork は独自のファイル名 `<game>/save/rfvp_sNNN.bin` と `<game>/save/rfvp_global.bin` を使い続け、古い `<game>/save/sNNN.bin` も読み込めます。これらはゲームディレクトリのセーブで、`.shinki` には移しません。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
