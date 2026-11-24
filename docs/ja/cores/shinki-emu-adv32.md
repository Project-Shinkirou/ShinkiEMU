---
title: Shinki ADV32
description: CIRCUS ADV32 コアの開発状況とローカルでの実行方法。
---

# Shinki ADV32

`shinki.emu.adv32` は CIRCUS ADV32 エンジンのコアです。ゲームの `AdvData` から MES 命令、CRX 画像、CRM/PCK アーカイブ、音声を読み込み、C ABI v3 経由で CPU フレームを渡します。デスクトップでは単体の動的ライブラリとして使用できます。

シナリオ、選択肢、メニュー、ムービーを実行し、対応する ADV32 形式のセーブを読み書きします。セーブの初期設定やファイル名の接頭辞、楽曲リスト、一部の鑑賞データを読み込むため、オリジナル版の実行ファイルをゲームディレクトリに残してください。ゲームファイルは同梱しません。未対応の命令に到達すると、スクリプト名とバイト位置を報告します。

既定では CPU で合成します。Windows、Linux、macOS、Android、iOS では `hardware_compositing` を有効にすると、シーンのレイヤーと文字を GPU で合成します。連続アニメーション中に処理中となる GPU フレームは最大 1 枚です。C ABI v3 では CPU への読み戻しが必要で、動画のデコードやピクセル単位のマスク処理も CPU が行います。OpenHarmony では CPU 合成と OHAudio を使います。

実行ファイルが指定するフォントがインストール済みであれば、そのフォントを使います。`font_file` を設定すると、ゲームディレクトリ内のフォントを指定できます。

スクリプトが起動時認証を要求した場合、Windows では実行ファイルに記録されたボリューム名を、マウント中の光学メディアと照合します。レジストリを使うディスクレス認証には対応しません。起動時の確認を省略する場合は `skip_disc_check` を明示的に有効にしてください（既定は無効）。headless では `[{"id":"skip_disc_check","value":{"Bool":true}}]` を `--configuration` で渡します。

デスクトップ、Android、iOS、OpenHarmony のローカル Manager ビルドでは `--features static-adv32` で組み込めます。これらは組み込み候補であり、ビルドや実機プレイの確認結果ではありません。公開版 Manager には含めず、単体配布にも各プラットフォームとプレイテストの確認が必要です。

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

セーブの確認には `--copy-out AdvData/SAVEDATA` を使えます。正常に終了すると、実行用のコピーからセーブディレクトリを取り出し、出力先の `game-copy-out` に保存します。
