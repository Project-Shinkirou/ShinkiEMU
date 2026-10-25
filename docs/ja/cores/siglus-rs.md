---
title: Siglus
description: Siglus のフォーマット、ネイティブ動作、制約。
---

# Siglus

`plugin_id: siglus_rs` · エンジンファミリー: SiglusEngine

上流：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)。適応 fork：[Project-Shinkirou/siglus_rs の shinki ブランチ](https://github.com/Project-Shinkirou/siglus_rs/tree/shinki)。

## フォーマット

`Scene.pck` は scene チャンクとパッケージ単位の include プロパティ・コマンドをまとめたものです。パッケージのオフセットはパッケージの先頭からの相対値で、デコード後の `.ss` のオフセットはチャンクの先頭からの相対値です。文字列インデックスのオフセットと長さはバイトではなく UTF-16 のコード単位で数えます。格納された文字列には、インデックスに依存する word 単位の変換がかかっています。scene、label、command entry の各テーブルは、それぞれ VM の制御フローに関わります。

## ネイティブ動作と制約

fork は Siglus の VM、レンダラー、テキスト・キーボード入力、ネイティブのセーブを保持します。アダプターが実行ループを担当し、CPU フレームを公開します。ネイティブの設定では、original/chs の対になったリソース、G01/movie の選択、独立したセーブルートを扱えます。

ローカライズ版の wrapper は組み込みランタイムの一部ではありません。コアは wrapper の機械語パッチを適用せず、不足しているフォントを補うこともしません。パッケージのデコード、リソースの設定、実行時の動作は、それぞれのリリースごとに確認する必要があります。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
