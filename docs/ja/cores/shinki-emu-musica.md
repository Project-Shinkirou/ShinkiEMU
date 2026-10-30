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

オリジナル版の進行状況や設定を引き継ぐには、`Documents\minori\<title>\` のファイルを `.shinki/musica/minori/<title>/` にコピーします。オリジナル版の exe で続けるときは同じようにコピーし戻します。`sysparams.txt` のうち Musica が使わないキー（フォント名やウィンドウサイズなど）はそのまま残ります。オリジナル版に対応するキーがない設定は、同じ形式で `.shinki/musica/musica-settings.txt` に保存します。eden ではキャラクターごとの 5 つのボイス切り替えがここに入ります。

圧縮ファイルの先頭には、Perseus の `SerialCode` から求めたプレフィックスが付きます。Windows では `HKLM\Software\minori\perseus` の `SerialCode` を読み、その CRC32 を使います。値がなければフォルダー内の既存のオリジナル版ファイルのプレフィックスを流用し、それもなければ 0 を書きます。eden は常に 0 です。

既知の違い：`system.dat`、`sysparams.txt`、ファイルのプレフィックスはオリジナル版とバイト単位で一致します。zlib で圧縮された `.tbr` と `cgv.dat` は、展開後の内容は同じですが、Musica の deflate エンコーダーはオリジナル版の zlib と異なる選択をするため、圧縮後のバイト列は通常一致しません。オリジナル版の exe はこれらのファイルを問題なく読み込めます。Musica は内容が変わったときだけファイルを書き直すので、コピーしたファイルは新しい既読フラグや CG が記録されるまで元のまま残ります。

以前の Musica が `.shinki/musica/saves/` に JSON で保存していた設定、クイックセーブのカーソル、グローバルな進行状況は、一度だけネイティブファイルに取り込まれます。既存のネイティブファイルは上書きせず、取り込んだ JSON は `*.imported` に名前を変えます。JSON が別のゲームのものだったり壊れていたりすると、取り込みは中止され、どのファイルも変更されません。認識できない exe の場合、Musica はネイティブファイルを書きません。古い JSON と `.asav` スロットは引き続き読み込み、`SHINKI_EMU_MUSICA_SAVE_TITLE` を一度だけログに出し、設定と進行状況の変更はそのセッションの間だけ保持します。

製品独自のスロットは `.shinki/musica/saves/` にあり、archive/profile/localization/font の識別情報に紐づけられます。初回利用時に旧 `.shinki-musica/saves/` のファイルを移動し、新しい場所の既存ファイルは上書きしません。その後は新しい場所だけを読み込みます。無効な候補が実行中の状態を変えたり、既存のファイルを上書きしたりすることはありません。オリジナル版 eden のセーブとの相互利用は、明示的に対応したメッセージのチェックポイントに限られ、オリジナル版のセーブ全般との互換性を意味するものではありません。

[ReMinori](reminori.md) は別の実装で、実行部、設定、セーブはそれぞれ独立しています。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
