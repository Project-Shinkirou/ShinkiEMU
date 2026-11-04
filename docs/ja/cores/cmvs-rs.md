---
title: cmvs-rs
description: 外部 CMVS/CVNS コアの上流対応範囲、設定、ホストの制約。
---

# cmvs-rs

`plugin_id: cmvs-rs` · エンジンファミリー：CMVS / CVNS

[Shizuku-in/cmvs-rs](https://github.com/Shizuku-in/cmvs-rs) を[組織 fork](https://github.com/Project-Shinkirou/cmvs-rs/tree/shinki) 経由で利用する外部コアです。ShinkiEMU 独自の [`shinki.emu.cmvs`](shinki-emu-cmvs.md) と共存します。一方を選択しても、もう一方の実装や設定は変わりません。VM、描画、互換性、セーブ方針はそれぞれ独立しています。

上流に登録されたローカルリリースを確保できず、実作品のフレーム検証が未完了のため公開を保留しています。HAPYMAHER_RE は起動スクリプトの hash が異なり、HAPYMAHERFD_RE は上流に登録がありません。登録変更や profile の強制指定による回避は行いません。

## 対応範囲

上流に登録された CMVS/CVNS 作品のみを対象とします。対象リリース、形式の分岐、実行上の制約は[上流の登録一覧](https://github.com/Shizuku-in/cmvs-rs/blob/b9fed68ba05f80444338331bb3999f14c8f4f976/crates/cmvs-core/profiles/games.txt) と[状態報告](https://github.com/Shizuku-in/cmvs-rs/blob/b9fed68ba05f80444338331bb3999f14c8f4f976/docs/status.md) を参照してください。PS2A プローブの一致はエンジンデータの検出であり、動作保証ではありません。未登録のデータや実行ファイルは open 時に上流が拒否します。アダプターはその判定を保持し、作品ホワイトリストを追加しません。

worker は上流の `Startup`、CPU/フォント描画、CMV/MGV 動画デコーダー、ネイティブ Vorbis 音声、スナップショットとセーブの実装を利用します。フレームリースは完成した不変のエンジン画面を所有し、close 後も読み取り・返却できます。動的ビルドは C ABI v3 query を公開し、静的ビルドは衝突するシンボルを公開せず Rust パスの query を提供します。

## 設定とセーブ

- `encoding`：上流の `auto`、`shift-jis`、`gbk`、`utf-8`。自動判定が曖昧な場合はエラーになります。
- `profile`：研究用の上流エンジン profile 指定。空欄は上流による自動判定です。
- `font` / `font-face`：ユーザーが用意する OpenType/TTC フォントと面番号。上流のフォント描画は Shift-JIS が必要です。空欄の場合は上流の文字出力バリアが残ります。フォントは同梱しません。
- `user-data-dir`：ゲームディレクトリー外の任意の上流ユーザーデータディレクトリー。既定値は空欄で、上流へそのまま渡します。管理セーブルートや `.shinki` へのリダイレクトを追加しません。未指定時は上流の書き込みバリアが残ります。既存セーブの移行・上書きは行いません。
- `block-unreviewed` / `block-presumed`：上流コマンドゲート。どちらも既定値は false です。許可されることと互換性が確認されることは別です。

## ホスト ABI の制約

C ABI v3 にはカスタムカーソル画像、ディスプレイの列挙・選択、縦横比・フィルター方針を伝えるフィールドがありません。外観だけのカーソル要求は Host の既定カーソルを使用し、診断を一度出力します。それ以外の表現できない要求は、不足する機能を示して `UNSUPPORTED` を返します。ダイアログはキャンセル可能なホストサービスを使用し、headless ホストは回答を捏造しません。コア自体はネイティブゲームウィンドウを作成しません。

セーブ、key、設定の場所と形式は上流のままです。バイナリーと対応ソースには MIT/Apache-2.0 ライセンス、第三者通知、変更声明を含めます。正確な pin とターゲット別制約は [`build/cores.toml`](../../../build/cores.toml) に記録します。
