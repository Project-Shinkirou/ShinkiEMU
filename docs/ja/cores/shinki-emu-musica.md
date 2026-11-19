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

コアの owner が SC VM、ハードウェアによるオフスクリーン合成、ネイティブ音声を実行します。既定の `launch_mode=direct` は設定された元の入口 SC を実行し、`title` にはリリースごとのフロントエンドデータが必要です。入力とフレームのポーリングでは VM は進みません。論理キャンバスとホストの拡大縮小は別設定です。設定画面とセーブカードは復元済みのレイアウトを維持し、タイトルの当たり判定と鑑賞リストは幅 1280 に依存しません。動画 ASS は実際の movie クロックと明示したフォントを使い、BGM 字幕には対応しません。

未知のコマンド、未対応の演出、デバイスの故障は明示的にエラーを報告します。音声は現在、上限付きでリソース全体をデコードしており、任意の長さのメディアをストリーミングでデコードするものではありません。

## フロントエンドデータとアーカイブ検出

元ファイルの解析、リリースに紐付いた Metadata 設定、非標準リソースの Luau 修正の順に取得します。作品名の分岐、固定 CLEAR フラグ、回想スクリプト表、動画名、登場人物表は使いません。

| データ | 元データと現在の対応範囲 | 取得できない場合の設定 |
| --- | --- | --- |
| タイトル、操作、矩形、切替と解放条件 | SC のメニューは元の入口を実行します。復元済み PAZ 索引の画像名だけから操作・矩形・変数は分かりません。ネイティブ EXE のメニュー表は未解析で、画像名のパターンによる推測は行いません。 | `frontend.titles` に実際の背景/ホバー URI、ボタン、グローバル変数条件 |
| 回想 | `frontend.replay_index` で指定した元の SC 索引から `.chain file.sc` を元の順で読みます。通常のシナリオ名では分類しません。この索引リーダーは `#label` 付きの対象を扱いません。 | `frontend.replays` に順序付きラベル、スクリプト URI、条件 |
| 動画 | 元の SC ディレクトリから movie/wait/end のみの独立した `.movie` スクリプトを検出し、動画の存在も確認します。`movie_index` で元の `.chain file.sc` 索引も指定できます。動画を含む通常のシナリオは除外します。 | `frontend.movies` に追加初期化が必要なリリースのスクリプト入口 |
| 音楽/CG | 実際の `bgm` アーカイブの `.ogg` を音楽一覧に使います。通常の画像ディレクトリから CG と背景は区別できません。 | 元の音楽がない場合は `frontend.bgm`、CG 索引は `frontend.cg` |
| 登場人物 | 実際の `voice` アーカイブの `.ogg` 名から `-`/`_` より前の部分を取得します。区切りがなければ拡張子と末尾の数字を除きます。名前の意味は解釈しません。 | `voice_characters` JSON 配列 `{prefix,label,enabled}` で名前・順序・初期設定を補足 |
| アーカイブの役割 | `key.toml` の実際の `archive_keys`。probe/open は同じ PAZ 0/1/2 構造・復号・索引とエントリの境界検証を使います。 | `archive_keys.<role>.required` は既定で **true**。任意のアーカイブは false。欠落は省略し、存在する壊れたファイルはエラー |

自動 probe はゲームにある既定の `key.toml` を使い、マウント後に解析できる SC コマンドを確認します。暗号化 PAZ に共通マジックはないため、ファイル名だけでは識別しません。既定の鍵がなければ自動一致せず、外部 `profile_file` の明示選択では同じ検証を使います。probe は鍵をインストールしません。CLI インポートの宣言済み役割は既定で `required=true`、keys.v1/profile.v1 の明示的な `required=false` は保持します。GARbro に任意フラグはないため、出力された私有 key.toml を明示的に編集します。必須ファイルの欠落は役割名と期待パスを報告します。

descriptor は中国語ラベル付きで `frontend`（JSON 文字列、既定は空）、`voice_characters`（JSON 文字列、既定は `[]`）、既存のキャンバス/起動設定を宣言します。リリース設定は `Metadata/engines/presets/<rid>/shinki.emu.musica.json` の `shinki.emu.presets.v1` に置きます。`configuration.frontend` は次のオブジェクトを文字列に符号化したものです。

```json
{
  "titles": [{
    "background": "musica:/sys/welcome.png",
    "hover": "musica:/sys/highlight.png",
    "requires": {},
    "buttons": [
      {"rect": [10, 20, 120, 40], "action": "new_game"},
      {"rect": [10, 80, 120, 40], "action": "memories"},
      {"rect": [10, 140, 120, 40], "action": "exit"}
    ]
  }],
  "replay_index": "musica:/scr/memory-index.sc",
  "replays": [],
  "movies": [{"label": "Credits", "resource": "musica:/scr/credits.sc", "requires": {"finished": 1}}],
  "cg": [{"label": "Memory", "resource": "musica:/bg/memory.png"}]
}
```

タイトルは `requires` が全て一致する最後の要素を使い、未定義のグローバル変数はゼロとします。ボタンは論理 `[x,y,width,height]` と `new_game`、`load`、`config`、`memories`、`exit` を指定します。鑑賞項目は `label`、`resource`、任意の `requires` を持ち、空の分類は表示しません。方向キーで選択、Enter/Space で実行、Escape で戻り、Backspace で鑑賞音楽を停止します。テキストとポインターは同じキャンバス比率の 12 行ページを使います。固定の曲番号や CG ページ数はありません。

各 JSON フィールドは ABI の 4096 バイト上限に従います。大きな一覧は元の SC 索引を指定してください。動画の自動走査は 4096 スクリプト、展開ソース合計 64 MiB までで、それ以上は `movie_index` が必要です。検出できた場合は元の所属・順序を優先し、同じ URI の preset ラベルと解放条件は保持します。SC の条件分岐から解放式を復元する機能はなく、条件は preset で記述します。

**設定移行：**保存済み Manager 設定の `voice_ren`、`voice_sui`、`voice_aya`、`voice_tou`、`voice_mot` を削除し、`voice_characters` に実際の接頭辞を指定します。旧フィールドの別名は残しません。明示的な `launch_mode=title` には `frontend` が必要で、元の SC メニューなら `direct` に切り替えます。eden/Perseus は実際の画像・解放条件・回想/動画入口・人物順をリリース preset に置き、コアの特例にはしません。合成 fixture はタイトル画像名の 2 通りの大文字小文字と、任意アーカイブを欠いた別の 640×360 データを検証し、商用リリースの互換性検証とは区別します。ネイティブ形式の 5 個の音声設定キーは宣言順に対応付け、追加の人物はフロントエンド設定 `voice_<prefix>` を使います。同じ人物集合と順序について、旧対応とのエンコード結果をバイト単位で比較する回帰テストがあります。

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

`directory` はゲームからの相対パス、または Documents を含むオリジナル版の絶対パスです。Documents の自動検索は行いません。`system_format` は `Rc4` または `Blowfish`、`extended` は復元済み拡張チェックポイント形式とスロット群を選び、作品名とは無関係です。`append_slot_prefix` は元ファイルにまだない追加シナリオ用の接頭辞を補います。`voice_keys` は設定順の最初の 5 人をネイティブ設定キーに対応付け、追加の人物はフロントエンド設定キーを使います。未対応の記述バージョンはエラーになります。

元ファイルから分かる値を優先し、レジストリキー、追加の接頭辞、設定キー、コマンド周期を記述で補います。解決した記述は `.shinki/musica/save-identity.json` に保存し、スロット作成前でも再起動後の設定を維持します。フロントエンド設定の移行は前節を参照してください。旧 minori ディレクトリは移動せず、完全な識別情報を取得できない場合は `SHINKI_EMU_MUSICA_SAVE_CONFIG` で `native_save` の指定を求めます。

Windows では `serial_key` が指す HKLM キーの ANSI `SerialCode` バイト列（最初の NUL まで）の CRC32 を使います。取得できない場合は明示的な `prefix`、検証済み圧縮ファイルのプレフィックス、最後に厳密照合なしのゼロを使います。4 バイトの値からレジストリキーや保存先を逆算することはできません。eden 形式なら `eden` / `edenPM`、`eden_en 1.00`、RC4、基本スロットを、Perseus 形式なら `perseus_`、`perseus 1.00`、Blowfish、拡張スロットとレジストリキーをリリース設定で指定します。コア内に作品表はありません。

対応済みの形式ではグローバル変数、設定、CG、既読情報、スロットをネイティブ形式で保存します。明示したオリジナル版のディレクトリには直接書き込み、従来の `.shinki/musica/minori/<folder>/` はそのまま使います。対応ファイルは元のインストール先と相互にコピーできます。`system.dat`、`sysparams.txt` とプレフィックスは元の表現を維持します。zlib の圧縮バイト列が異なっても展開内容は同じです。未使用の設定バイトは保持し、フロントエンド専用設定は `.shinki/musica/musica-settings.txt` に保存します。

**ネイティブ識別情報も記述も取得できない場合**でも、完全なバックエンドを作り、全状態を `<game>/.shinki/musica/generic-v1/` に実際に保存します。ゲームディレクトリごとに分離されます。`slot_NNNN.sav` は独自の `shinki.musica.save.v1` 署名と復元済み基本チェックポイント本文を使います。補助ファイルは RC4 のグローバル表、タブ区切り設定、ゼロプレフィックス付き zlib の CG/既読表です。これは ShinkiEMU の汎用形式であり、未知のオリジナル版で読めるという意味ではありません。既知の exe 名だけでは選択は変わりません。記述の切り替えで保存先が変わっても、以前のファイルは削除も統合もしません。バックアップを残してください。

旧 `.shinki/musica/saves/` の JSON 設定、クイックカーソル、グローバル進行状況は、**両方のバックエンド**へ一度だけ、上書きせず取り込みます。書き込み成功後にのみ元ファイルを `.imported` に改名します。既存の保存先が優先されます。旧 beta `.asav`（`AMUSSV06` 以前）は未対応のまま、両方の旧保存先で変更せず保持します。バックアップ後に新しいスロットを作成してください。

旧進行ハッシュは元の入口 SC の `.setglobal` 変数名、`frontend` の条件キー、preset の `legacy_progress_flags` JSON 文字列配列（既定 `[]`）の順で対応付けます。固定の変数名表はありません。未解決でも起動を妨げず、対応済み項目を既存のグローバル値を上書きせず取り込みます。元 JSON は変更せず、`.shinki/musica/saves/global-progress-unmapped.json` に未解決ハッシュとゲームディレクトリを記録します。宣言を補って再起動すれば保留中の取り込みを再開し、その間の値の変更を維持します。全項目の解決後にのみ元ファイルを `.imported` に改名します。保留診断がなければ既存のネイティブ保存先が引き続き優先されます。実行時の進行記録は実際の `.setglobal` の値が 1 かどうかで決まり、変数名の綴りには依存しません。メニュー条件は設定された整数値で判定します。

旧 `configuration.json` に `character_voice_enabled` がある場合は、リリースの宣言順を使って `character_voices` オブジェクト配列へ明示的に変換してください。変換前の原本はバックアップとして残し、厳密なインポーターが 5 人の一覧を推測することはありません。

チェックポイントはスクリプト変数、背景、合成前景、立ち絵の位置とオフセット、BGM、履歴、ノベル形式（`CP1=2`）のページと `L4_n` の位置を復元します。ロード時はメッセージと音声を最初から再開します。再生時計、翻訳文、メニューの選択位置は保存しません。未対応の効果や追加レイヤーは拒否し、内部スナップショットには切り替えません。

基本形式はオート 0、クイック 1–9、`AppendPlaying` で設定された追加名前空間を選びます。拡張形式はクイック 0–9、オート 10–19、手動 20 以降です。対応する元ファイルや記述により eden/Perseus の循環動作を維持します。オートセーブは設定されたコマンド周期、4 項目以下の選択肢の直前、動画の直前に行います。`exitFromEvent`、`prohibitAutoSave`、`prohibitSaveLoad` の禁止条件は維持します。サムネイル、スロットの順に、それぞれアトミックに置き換えます。

[ReMinori](reminori.md) は別の実装で、実行部、設定、セーブはそれぞれ独立しています。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
