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

Musica は exe 名や作品名ではなく、オリジナル版のファイルからセーブ識別情報を解析します。対象はゲーム直下、`save/`、`savedata/`、既存の `.shinki/musica/minori/*/` です。デコード可能な `system.dat` から RC4/Blowfish の形式を、番号付き `.sav` から実際のディレクトリ、ファイル名の接頭辞と桁数、NUL 終端の署名、4 バイトのプレフィックス、基本/拡張チェックポイント形式を取得します。追加シナリオの名前空間を推定するには、両方の命名形式のファイルが必要です。署名や SerialCode だけでは他のパラメーターは決まりません。

| ファイル | 内容 |
| --- | --- |
| `system.dat` | グローバルフラグとクリア状態 |
| `sysparams.txt` | 設定、クイックセーブとオートセーブのカーソル（タブ区切り、CRLF） |
| `<script>.tbr` | スクリプトごとの既読メッセージのビットマップ |
| `cgv.dat` | 閲覧済み CG の一覧 |
| `<prefix><number>.sav` | 解析または設定された命名規則によるメッセージ位置と履歴 |
| 同名の `.png` | 不透明 RGB の領域平均サムネイル：基本形式 128×80、拡張形式 106×60 |

元ファイルだけで形式が決まらない場合、リリースに紐付いた Metadata preset（`Metadata/engines/presets/<rid>/<plugin_id>.json`、`shinki.emu.presets.v1`）の `configuration.native_save` に JSON **文字列**で記述を指定します。デコード後の例：

```json
{
  "version": 1,
  "directory": "save",
  "slot_prefix": "slot_",
  "append_slot_prefix": null,
  "slot_digits": 4,
  "signature": "engine-release 1.00",
  "system_format": "Rc4",
  "extended": false,
  "serial_key": null,
  "prefix": 0,
  "auto_save_cycle": 200,
  "voice_keys": null
}
```

`directory` はゲームからの相対パス、または Documents を含むオリジナル版の絶対パスです。Documents の自動検索は行いません。`system_format` は `Rc4` または `Blowfish`、`extended` は復元済みの拡張チェックポイント形式とクイック/オートのスロット群を選びます。作品名とは無関係です。`append_slot_prefix` は元ファイルにまだない追加シナリオ用の接頭辞を補います。`voice_keys` は既存の 5 つの音声グループとネイティブ設定キーの対応です。この変更ではフロントエンドのリソース対応は変えません。未対応の記述バージョンはエラーになります。

元ファイルから分かる値を優先し、レジストリキー、追加の接頭辞、設定キー、コマンド周期を記述で補います。解決した記述は `.shinki/musica/save-identity.json` に保存し、スロット作成前でも再起動後の設定を維持します。既存の設定フィールドは削除していません。旧 minori ディレクトリは移動しません。旧ファイルから完全な識別情報を取得できない場合は `SHINKI_EMU_MUSICA_SAVE_CONFIG` で `native_save` の指定を求め、旧進行状況を黙って隠すことはありません。

Windows では `serial_key` が指す HKLM キーの ANSI `SerialCode` バイト列（最初の NUL まで）の CRC32 を使います。取得できない場合は明示的な `prefix`、検証済み圧縮ファイルのプレフィックス、最後に厳密照合なしのゼロを使います。4 バイトの値からレジストリキーや保存先を逆算することはできません。eden 形式なら `eden` / `edenPM`、`eden_en 1.00`、RC4、基本スロットを、Perseus 形式なら `perseus_`、`perseus 1.00`、Blowfish、拡張スロットとレジストリキーをリリース設定で指定します。コア内に作品表はありません。

対応済みの形式ではグローバル変数、設定、CG、既読情報、スロットをネイティブ形式で保存します。明示したオリジナル版のディレクトリには直接書き込み、従来の `.shinki/musica/minori/<folder>/` はそのまま使います。対応ファイルは元のインストール先と相互にコピーできます。`system.dat`、`sysparams.txt` とプレフィックスは元の表現を維持します。zlib の圧縮バイト列が異なっても展開内容は同じです。未使用の設定バイトは保持し、フロントエンド専用設定は `.shinki/musica/musica-settings.txt` に保存します。

**ネイティブ識別情報も記述も取得できない場合**でも、完全なバックエンドを作り、全状態を `<game>/.shinki/musica/generic-v1/` に実際に保存します。ゲームディレクトリごとに分離されます。`slot_NNNN.sav` は独自の `shinki.musica.save.v1` 署名と復元済み基本チェックポイント本文を使います。補助ファイルは RC4 のグローバル表、タブ区切り設定、ゼロプレフィックス付き zlib の CG/既読表です。これは ShinkiEMU の汎用形式であり、未知のオリジナル版で読めるという意味ではありません。既知の exe 名だけでは選択は変わりません。記述の切り替えで保存先が変わっても、以前のファイルは削除も統合もしません。バックアップを残してください。

旧 `.shinki/musica/saves/` の JSON 設定、クイックカーソル、グローバル進行状況は、**両方のバックエンド**へ一度だけ、上書きせず取り込みます。書き込み成功後にのみ元ファイルを `.imported` に改名します。既存の保存先が優先されます。旧 beta `.asav`（`AMUSSV06` 以前）は未対応のまま、両方の旧保存先で変更せず保持します。バックアップ後に新しいスロットを作成してください。

チェックポイントはスクリプト変数、背景、合成前景、立ち絵の位置とオフセット、BGM、履歴、ノベル形式（`CP1=2`）のページと `L4_n` の位置を復元します。ロード時はメッセージと音声を最初から再開します。再生時計、翻訳文、メニューの選択位置は保存しません。未対応の効果や追加レイヤーは拒否し、内部スナップショットには切り替えません。

基本形式はオート 0、クイック 1–9、`AppendPlaying` で設定された追加名前空間を選びます。拡張形式はクイック 0–9、オート 10–19、手動 20 以降です。対応する元ファイルや記述により eden/Perseus の循環動作を維持します。オートセーブは設定されたコマンド周期、4 項目以下の選択肢の直前、動画の直前に行います。`exitFromEvent`、`prohibitAutoSave`、`prohibitSaveLoad` の禁止条件は維持します。サムネイル、スロットの順に、それぞれアトミックに置き換えます。

[ReMinori](reminori.md) は別の実装で、実行部、設定、セーブはそれぞれ独立しています。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
