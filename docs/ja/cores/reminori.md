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

ShinkiEMU に組み込んだ ReMinori は `<game>/userdata/` を使用します。上流の単体版では実行ファイルと同じ場所にある `userdata/` を使用します。作成や書き込みができない場合は、上流と同じ `Documents/eden/userdata/` にフォールバックします。ここに `edenNNNN.sav` と `edenNNNN.png` のサムネイル、クイックセーブの `edenPMNNNN.sav`、暗号化・圧縮された進行状況の `system.dat`、`settings.tsv`、`readflags.bin`、`cgseen.tsv` が保存されます。デコードした動画は上流と同じキャッシュ `<OS のキャッシュディレクトリ>/eden/movies/` に置かれます。

CP932 だけのセーブはネイティブの形式を保ちます。GBK スクリプトを使うセッションは、本文とコメントを保つために ReMinori の UTF-8 port セーブ形式を使います。このセーブが元の eden で読めるとは限りません。オーディオデバイスが失敗しても、診断用の NullAudio に黙って切り替えることはありません。ネイティブのモーダルには、動作する型付きのホストサービスが必要です。

元の Windows 版 eden は `Documents\minori\<タイトル>\` にセーブを保存します。英語版は `Documents\minori\eden (English edition)\` です。ReMinori で使うには、`edenNNNN.sav`、`edenNNNN.png`、`system.dat` を使用中の `userdata/` にコピーしてください。互換性のある市販版形式のセーブを元の eden に戻すときは逆方向にコピーします。両方のディレクトリをバックアップし、コピー先の既存ファイルは上書きしないでください。

beta.1/beta.2 で `save_directory` を選択していた場合は、その中のファイルを使用中の `userdata/` に手動でコピーし、既存ファイルは上書きしないでください。

eden 向けの実装なので、minori のすべての作品やリリースに当てはまるわけではありません。[リポジトリ内の Musica コア](shinki-emu-musica.md)の実行部を置き換えるものではなく、両者の設定とセーブの識別はそれぞれ独立しています。ReMinori は上流の AGPL-3.0 の義務を引き継ぎます。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
