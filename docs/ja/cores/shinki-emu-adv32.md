---
title: Shinki ADV32
description: CIRCUS ADV32 コアの開発状況とローカルでの実行方法。
---

# Shinki ADV32

`shinki.emu.adv32` は CIRCUS ADV32 エンジンのデスクトップ向け開発プレビューです。`AdvData` から MES 命令、CRX 画像、CRM/PCK アーカイブ、音声を読み込み、C ABI v3 経由で CPU フレームを渡します。一般配布やモバイル版 Manager への組み込みは行っていません。

シナリオ、選択肢、メニュー、ムービー、オリジナル版のセーブデータの一部に対応しています。公開には VM 命令の網羅とセーブデータの互換性の確認が必要です。正規に入手したゲームを使用し、インストール先にオリジナル版の実行ファイルを残してください。セーブの初期設定やファイル名の接頭辞、楽曲リスト、一部の鑑賞データを読み込むために使います。ゲームファイルは同梱しません。未対応の命令に到達すると、スクリプト名とバイト位置を報告します。

既定では CPU で合成します。**Hardware compositing**（`hardware_compositing`）を有効にすると、シーンのレイヤーと文字が GPU で合成されます。連続アニメーション中に処理中となる GPU フレームは最大 1 枚です。C ABI v3 では引き続き CPU への読み戻しが必要で、動画のデコードやピクセル単位のマスク処理も CPU が行います。速度向上の計測結果はまだありません。合成と読み戻しをまとめて GPU に送る共通レンダラーの変更は、このプレビューには含まれません。

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

セーブの確認には `--copy-out AdvData/SAVEDATA` を使えます。正常に終了すると、実行用のコピーからセーブディレクトリを取り出し、出力先の `game-copy-out` に保存します。
