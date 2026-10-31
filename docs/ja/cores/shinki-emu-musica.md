---
title: Musica
description: Musica のフォーマット、ネイティブ動作、制約。
---

# Musica

`plugin_id: shinki.emu.musica` · エンジンファミリー: Musica

ShinkiEMU リポジトリ内の自社コアで、独立した外部 fork はありません。実装は [Source/Families/shinki-emu-musica](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/Families/shinki-emu-musica)、元のコミットは [ORIGINS](../../../ORIGINS.md) を参照してください。

## フォーマット

リポジトリ内の Musica コアは PAZ とネイティブの SC を読み込み、分割アーカイブ、役割ごとの鍵、変換、上限付きの展開を扱います。プライベートな `key.toml` は `shinki.emu.musica.keys.v2` を使い、アルゴリズムのパラメーターと鍵をどちらも明示的に指定する必要があります。プローブで認識しても鍵がインストールされることはありません。標準のばらの SC と、設定した PNG の置き換えファイルは、ゲームのルートディレクトリ内に限られます。

## ネイティブ動作と制約

コアの owner が SC VM、ハードウェアによるオフスクリーン合成、ネイティブの音声を実行し、既定ではタイトルメニューを表示してからシナリオに入ります。入力とフレームのポーリングでは VM は進みません。現在の表示には 1280×720 のレイアウトと Noto Sans JP フォントの割り当てが含まれ、ホスト側で拡大縮小してもゲームの座標は変わりません。動画の ASS は実際の movie クロックと明示的なフォントを使い、このパスでは BGM 字幕のマッピングを受け付けません。

未知のコマンド、未対応の演出、デバイスの故障は明示的にエラーを報告します。音声は現在、上限付きでリソース全体をデコードしており、任意の長さのメディアをストリーミングでデコードするものではありません。

## セーブ

ゲームディレクトリに `eden_en.exe` または `perseus.exe` があると、Musica はオリジナル版の minori ファイルを `<game>/.shinki/musica/minori/<title>/` にバイト単位で互換な形式で読み書きします。オリジナル版は同じファイルを `Documents\minori\<title>\` に置きます。Musica が Documents に書き込むことはありません。タイトルのフォルダー名は eden が `eden (English edition)`、Perseus が `夏空のペルセウス` です。オリジナル版の Perseus はこの名前を ANSI API に渡すため、コードページが CP932 でない Windows では Documents 内のフォルダー名が文字化けします（GBK では `壞嬻偺儁儖僙僂僗`）。

| ファイル | 内容 |
| --- | --- |
| `system.dat` | グローバルフラグとクリア状態 |
| `sysparams.txt` | 設定、クイックセーブとオートセーブのカーソル（タブ区切り、CRLF） |
| `<script>.tbr` | スクリプトごとの既読メッセージのビットマップ |
| `cgv.dat` | 閲覧済み CG の一覧 |
| `edenNNNN.sav` / `edenPMNNNN.sav`、`perseus_NNNN.sav` | ネイティブのメッセージ位置と履歴 |
| 同名の `.png` | 不透明 RGB の領域平均サムネイル：eden 128×80、Perseus 106×60 |

オリジナル版の進行状況や設定を引き継ぐには、`Documents\minori\<title>\` のファイルを `.shinki/musica/minori/<title>/` にコピーします。オリジナル版の exe で続けるときは同じようにコピーし戻します。`sysparams.txt` のうち Musica が使わないキー（フォント名やウィンドウサイズなど）はそのまま残ります。オリジナル版に対応するキーがない設定は、同じ形式で `.shinki/musica/musica-settings.txt` に保存します。eden ではキャラクターごとの 5 つのボイス切り替えがここに入ります。

圧縮ファイルの先頭には、Perseus の `SerialCode` から求めたプレフィックスが付きます。Windows では `HKLM\Software\minori\perseus` の `SerialCode` を読み、その CRC32 を使います。値がなければフォルダー内の既存のオリジナル版ファイルのプレフィックスを流用し、それもなければ 0 を書きます。eden は常に 0 です。

既知の違い：`system.dat`、`sysparams.txt`、ファイルのプレフィックスはオリジナル版とバイト単位で一致します。zlib で圧縮された `.tbr` と `cgv.dat` は、展開後の内容は同じですが、Musica の deflate エンコーダーはオリジナル版の zlib と異なる選択をするため、圧縮後のバイト列は通常一致しません。オリジナル版の exe はこれらのファイルを問題なく読み込めます。Musica は内容が変わったときだけファイルを書き直すので、コピーしたファイルは新しい既読フラグや CG が記録されるまで元のまま残ります。

以前の `.shinki/musica/saves/` にある JSON 設定、クイックセーブのカーソル、グローバルな進行状況は、既存ファイルを上書きせず一度だけネイティブファイルへ取り込みます。認識できない exe ではネイティブスロットを読み書きできません。

実行中のセーブとロードはネイティブのバックエンドを使います。スクリプト変数、背景、合成前景、立ち絵の位置とオフセット、BGM、メッセージ履歴、ノベルモード（`CP1=2`）のページと `L4_n` の位置を復元します。ロード時は現在のメッセージと音声を最初から再開します。再生時計、翻訳文、メニューの選択位置は保存しません。未対応の効果や追加レイヤーは明示的に拒否し、内部スナップショットへ切り替えることはありません。

eden のオートスロットは 0、クイックスロットは 1–9 です。`AppendPlaying` が有効なら `edenPM` の名前空間を使います。Perseus はクイック 0–9、オート 10–19、手動は 20 以降です。オートセーブはネイティブのコマンド数周期、4 項目以下の選択肢の直前、動画の直前に実行します。ローカルの `exitFromEvent`、`prohibitAutoSave`、グローバルの `prohibitSaveLoad` が禁止条件です。`prohibitSaveLoad` は手動のセーブとロードも禁止します。サムネイル、圧縮スロットの順に、それぞれアトミックに置き換えます。

旧 beta の `.asav`（`AMUSSV06` 以前）には対応しません。`.shinki-musica/saves/` と `.shinki/musica/saves/` の旧ファイルは、読み込み、変換、移動、改名、削除を行いません。バックアップを残し、新しいネイティブスロットを作成してください。

[ReMinori](reminori.md) は別の実装で、実行部、設定、セーブはそれぞれ独立しています。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
