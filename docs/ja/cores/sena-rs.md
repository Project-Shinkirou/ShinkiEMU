---
title: Sena
description: Sena のフォーマット、ネイティブ動作、制約。
---

# Sena

`plugin_id: sena-rs` · エンジンファミリー: SoftPAL

上流：[xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs)。適応 fork：[Project-Shinkirou/sena-rs の shinki ブランチ](https://github.com/Project-Shinkirou/sena-rs/tree/shinki)。

## フォーマット

SoftPAL は、`SCRIPT.SRC` のバイトコードを、`POINT.DAT` の分岐・呼び出し先、`FILE.DAT` のリソース名、`TEXT.DAT` のテキスト、`MEM.DAT` の初期データとあわせて解釈します。`Sv20` ヘッダーは 12 バイトで、明示的なエントリー PC を含みます。命令とデータは little-endian の word を使います。extcall の引数はネイティブのポップ順で取り出す必要があり、ドキュメント上の表示順で読み取ってはいけません。

## ネイティブ動作と制約

SoftPAL の VM、待機の意味論、ネイティブの描画・音声、セーブは fork が担当します。ネイティブのロードに成功したあとも、続けてセーブできる状態でなければなりません。アダプターは `text_language=jp/cn/tc` を提供し、多言語フィールドの選択では元の日本語の単一フィールドの動作を保ちます。フォントはネイティブのゲームパスまたは明示的なネイティブ設定から取得し、描画サイズは ABI v3 を通じて要求します。

アダプターの対象範囲は引き続きネイティブ版と Steam 版の Koikake のパスで、すべての SoftPAL 作品に当てはまるわけではありません。未知の extcall や間接メモリのパスは実際のランタイムで確認する必要があり、データテーブルを認識できるだけでは不十分です。

## セーブパス

番号付きセーブは `<game>/save/saveNNN.dat` に保存されます。リバースエンジニアリング資料では、メタデータの `SaveFolder` が番号付きセーブのディレクトリであり、`VmTask_WriteSystemDat` が `SaveFolder\\system.dat` を書くことを確認しています。そのため出荷レイアウトでは `<game>/save/system.dat` になります（`reverse/GAME_VM_RE.md:489-492`、`reverse/IDB_CHANGELOG.md:782-801`）。fork 固有の `system.ini`、`system_mem.bin`、`read_lines.bin`、`history/` は元エンジンが作らない状態なので、`<game>/save/sena_rs/` に残ります。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
