---
title: Shinki ADV32
description: CIRCUS ADV32 コアの開発状況とローカルでの実行方法。
---

# Shinki ADV32

`shinki.emu.adv32` は CIRCUS ADV32 エンジンのコアです。ゲームの `AdvData` から MES 命令、CRX 画像、CRM/PCK アーカイブ、音声を読み込み、C ABI v3 経由で CPU フレームを渡します。デスクトップでは単体の動的ライブラリとして使用できます。

起動時に `AdvData/MES/start.mes` を探し、MES の方言（`0x9e97`、`0x9f9a`、`0xa6b9`、`0xaab6`、`0xabb6`）を判別します。検出結果はタイトルや版ごとの動作保証ではありません。セーブの初期値や名前の接頭辞、楽曲リスト、一部の鑑賞データを読み込むため、オリジナルの実行ファイルをゲームディレクトリに残してください。ゲームファイルは同梱しません。未対応の命令に到達すると、スクリプト名とバイト位置を報告します。

既定では CPU で合成します。Windows、Linux、macOS、Android、iOS では `hardware_compositing` を有効にすると、シーンのレイヤーと文字を GPU で合成します。連続アニメーション中に処理中となる GPU フレームは最大 1 枚です。C ABI v3 では CPU への読み戻しが必要で、動画のデコードやピクセル単位のマスク処理も CPU が行います。OpenHarmony では CPU 合成と OHAudio を使います。

実行ファイルが指定するフォントがインストール済みであれば、そのフォントを使います。`font_file` を設定すると、ゲームディレクトリ内のフォントを指定できます。

スクリプトが起動時認証を要求した場合、Windows では実行ファイルに記録されたボリューム名を、マウント中の光学メディアと照合します。レジストリを使うディスクレス認証には対応しません。起動時の確認を省略する場合は `skip_disc_check` を明示的に有効にしてください（既定は無効）。headless では `[{"id":"skip_disc_check","value":{"Bool":true}}]` を `--configuration` で渡します。

ADV32 は動的に読み込む単体コアとしてインストールします。配布前に、対象の版での VM 動作、オリジナルとのセーブ互換性、[ソース・ライセンス・DLL 差し替え手順](../../licenses/shinki-emu-adv32/WINDOWS-FFMPEG.md)を確認してください。

Windows の検証候補は Dal Segno（MES `0x9f9a`、セーブ接頭辞 `DS`）です。`AdvData/SAVEDATA` の `system.dat` は 19,736 バイト、各スロットは 72,440 バイトです。単体コアの検証を終えた版はまだありません。セーブはバックアップを取り、同じ版でコアとオリジナルの双方から読み込めることを確認してください。

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

セーブの確認には `--copy-out AdvData/SAVEDATA` を使えます。正常に終了すると、実行用のコピーからセーブディレクトリを取り出し、出力先の `game-copy-out` に保存します。
