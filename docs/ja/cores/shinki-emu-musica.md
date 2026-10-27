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

製品独自のスロットは `.shinki/musica/saves/` にあり、archive/profile/localization/font の識別情報に紐づけられます。初回利用時に旧 `.shinki-musica/saves/` のファイルを移動し、新しい場所の既存ファイルは上書きしません。その後は新しい場所だけを読み込みます。無効な候補が実行中の状態を変えたり、既存のファイルを上書きしたりすることはありません。オリジナル版 eden のセーブとの相互利用は、明示的に対応したメッセージのチェックポイントに限られ、オリジナル版のセーブ全般との互換性を意味するものではありません。未知のコマンド、未対応の演出、デバイスの故障は明示的にエラーを報告します。音声は現在、上限付きでリソース全体をデコードしており、任意の長さのメディアをストリーミングでデコードするものではありません。

[ReMinori](reminori.md) は別の実装で、実行部、設定、セーブはそれぞれ独立しています。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
