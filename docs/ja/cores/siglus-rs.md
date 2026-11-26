---
title: Siglus
description: Siglus のフォーマット、ネイティブ動作、制約。
---

# Siglus

`plugin_id: siglus_rs` · エンジンファミリー: SiglusEngine

上流：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)。適応 fork：[Project-Shinkirou/siglus_rs の shinki-headless-null-audio ブランチ](https://github.com/Project-Shinkirou/siglus_rs/tree/shinki-headless-null-audio)。

## フォーマット

`Scene.pck` は scene チャンクとパッケージ単位の include プロパティ・コマンドをまとめたものです。パッケージのオフセットはパッケージの先頭からの相対値で、デコード後の `.ss` のオフセットはチャンクの先頭からの相対値です。文字列インデックスのオフセットと長さはバイトではなく UTF-16 のコード単位で数えます。格納された文字列には、インデックスに依存する word 単位の変換がかかっています。scene、label、command entry の各テーブルは、それぞれ VM の制御フローに関わります。

## ネイティブ動作と制約

fork は Siglus の VM、レンダラー、テキスト・キーボード入力、ネイティブのセーブを保持します。アダプターが実行ループを担当し、CPU フレームを公開します。ネイティブの設定では、original/chs の対になったリソースと G01/movie の選択を扱えます。

scene の再起動後も、セッションの画像と動画のリソース言語を保持します。CHS モードは元の G00 ファイルを上書きせず、ネイティブの G01 と `mov_chs` の置換リソースを選択します。

Android Manager はゲーム起動時に別パッケージから Siglus コアを読み込みます。ディレクトリの検出ではメタデータだけを参照し、ネイティブライブラリは読み込みません。コアの有効化時に ABI 拡張経由で Android のコンテキストを渡します。Siglus は引き続き CPU フレームを生成します。

## セーブとキー

セーブはオリジナルのエンジンと同じ場所に書かれます。`<ゲーム>/savedata/` に `NNNN.sav` のスロットとサムネイル、`global.sav`、`config.sav`、`read.sav`、Twitter の状態が入り、キャプチャやエクスポートはゲームディレクトリからの相対パスに書かれます。復元したキーは上流と同じく `<ゲーム>/key.toml` に保存されます。`chs` のリソース対はオリジナルの対と `savedata/` を共有します。中国語パッチを当てたオリジナルも同じ動作です。ゲームディレクトリは書き込み可能である必要があります。

Android ではコアの配布順序 4 以降で、アプリ専用のファイルディレクトリと従来と同じゲームパスのハッシュから既定の保存先を求めます。既存のセーブは移動しません。明示した保存先は引き続き優先されます。

0.1.0-beta.1 と beta.2 はセーブを `%LOCALAPPDATA%\siglus_rs\saves\<ハッシュ>\siglus-<言語>-<ID>\`（macOS は `~/Library/Application Support/siglus_rs/saves/…`、Linux は `$XDG_DATA_HOME` または `~/.local/share/siglus_rs/saves/…`）に置いていました。ゲームを初めて起動したとき、コアはこのフォルダーを一度だけゲームディレクトリへ取り込みます。既存のファイルは上書きせず、古いフォルダーは `….imported` に名前を変えます。廃止された「セーブディレクトリ」オプションを設定していた場合、コアはそのフォルダーを見つけられません。中にある各 `siglus-*` フォルダーの内容（`savedata/` とキャプチャファイル）を、残したいファイルを上書きしないように手動でゲームディレクトリへコピーしてください。

ローカライズ版の wrapper は組み込みランタイムの一部ではありません。コアは wrapper の機械語パッチを適用せず、不足しているフォントを補うこともしません。パッケージのデコード、リソースの設定、実行時の動作は、それぞれのリリースごとに確認する必要があります。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
