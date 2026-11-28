---
title: Siglus
description: Siglus のフォーマット、ネイティブ動作、制約。
---

# Siglus

`plugin_id: siglus_rs` · エンジンファミリー: SiglusEngine

上流：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)。[ShinkiEMU のアダプターソース](https://github.com/Project-Shinkirou/siglus_rs/tree/575075a678a17d4e21ac81c24adbef124ab2027e) のリビジョンは `Cargo.lock` で固定しています。

## フォーマット

`Scene.pck` は scene チャンクとパッケージ単位の include プロパティ・コマンドをまとめたものです。パッケージのオフセットはパッケージの先頭からの相対値で、デコード後の `.ss` のオフセットはチャンクの先頭からの相対値です。文字列インデックスのオフセットと長さはバイトではなく UTF-16 のコード単位で数えます。格納された文字列には、インデックスに依存する word 単位の変換がかかっています。scene、label、command entry の各テーブルは、それぞれ VM の制御フローに関わります。

## ネイティブ動作と制約

fork は Siglus の VM、レンダラー、テキスト・キーボード入力、ネイティブのセーブを保持します。アダプターが実行ループを担当し、CPU フレームを公開します。ネイティブの設定では、original/chs の対になったリソースと G01/movie の選択を扱えます。

scene の再起動後も、セッションの画像と動画のリソース言語を保持します。CHS モードは元の G00 ファイルを上書きせず、ネイティブの G01 と `mov_chs` の置換リソースを選択します。

Android Manager はゲーム起動時に別パッケージから Siglus コアを読み込みます。ディレクトリの検出ではメタデータだけを参照し、ネイティブライブラリは読み込みません。コアの有効化時に ABI 拡張経由で Android のコンテキストを渡します。Siglus は引き続き CPU フレームを生成します。

Android コアの Oboe 音声には NDK の `libc++_shared.so` が必要です。コアパッケージには対象 ABI のライブラリと NDK のライセンスを含めます。

## セーブとキー

コアは専用の保存先に書き込みます。既定の起点は Windows では `%LOCALAPPDATA%`、macOS では `~/Library/Application Support`、Linux では `$XDG_DATA_HOME` または `~/.local/share`、Android ではアプリ専用のファイルディレクトリです。その下に `siglus_rs/saves/<正規化したゲームパスのハッシュ>/` を追加します。

`save_root` を指定すると、このパス全体を置き換えます。ゲームやオリジナルのセーブディレクトリとは別の絶対パスを指定してください。いずれの場合も、その下に `siglus-<original|chs>-<リソース対の識別子>/` が付きます。セーブと設定は `savedata/` に入り、キャプチャとエクスポートも同じ領域に書き込みます。original と CHS はセーブを自動で共有しません。

古いセーブを自動で取り込んだり、名前を変えたりはしません。別の場所にある互換セーブを試す場合は、ゲームを終了し、元のファイルを残したまま同じリソース対の保存先にコピーしてください。既存のファイルを上書きせず、再起動後に読み込めることを確認します。キーはローカルに保管してください。

ローカライズ版ランチャーの機械語パッチは適用しません。CJK フォントのフォールバックは欠けている文字を補いますが、元のゲームフォントを再現するものではありません。リソースと動作の互換性はリリースごとに確認してください。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
