---
title: メタデータとパッチ投稿
description: 互換性レポートおよび Luau パッチの投稿手順と、メンテナー審査・スナップショット生成仕様。
---

# メタデータとパッチ投稿

`Metadata/` は公式ポータルと ShinkiEMU Manager が共有する単一データソース（`https://project-shinkirou.github.io/data/shinkiemu/v1/`）です。互換性レポートと Luau パッチは GitHub のフォームから提出できます。

## 1. 互換性レポートの提出

[互換性レポートフォーム](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)に実測結果を記入します：

1. **VNDB ID の確認**：作品 URL から `vID`（例 `v17`）、対象リリース URL から `rID`（例 `r1234`）を取得します。
2. **正確な環境情報**：Manager バージョン（**設定 → バージョン情報**）、大文字・小文字を区別する正確なコア ID とバージョン（**コア**画面）、OS・CPU アーキテクチャ（`x86_64` / `aarch64`）・区分（`desktop` / `device` / `simulator`）を指定します。
3. **動作ステータス**：

| ステータス | 意味 |
|---|---|
| `perfect` | 映像・音声・演出・セーブ／ロードすべて問題なく動作 |
| `completable` | 軽微な不具合はあるが最後までプレイ可能 |
| `flawed` | 一部プレイできるが大きな不具合がある |
| `boot_only` | タイトルやメニューまでのみ起動 |
| `unplayable` | 起動しない、または即座にクラッシュ |

> [!CAUTION]
> 商用素材、本文、鍵、セーブデータは絶対に添付しないでください。ビルド成功やエンジン認識だけでは互換性レポートになりません。

各レポートは 5 つ組 `(release_id, emulator, platform, architecture, variant)` をキーに `Metadata/compatibility/entries/<vid>/<rid>/<plugin_id>/<platform>-<architecture>-<variant>.json` へ格納されます。

## 2. Luau パッチの作成と投稿

[Luau パッチ投稿フォーム](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)を使用します（詳細は [shinkiemu-patch](../../.agents/skills/shinkiemu-patch/SKILL.md) 参照）。

- **基本メタデータ**：安定した ASCII `id`、SemVer、`title`/`summary`（`zh-CN` と `en` 必須、`ja` 任意）、コア ID、出典 URL、ライセンス、必要に応じた許諾 URL。
- **リリース指紋（`releases`）**：VNDB の `release_id`・`vn_id`・版情報と元ファイル指紋（`path`, `size`, `sha256`）を宣言します。適用前に Manager が一致確認を行います。
- **ダウンロード宣言（`downloads`）**：`ctx.download(url, sha256, size)` を使う場合、すべての HTTPS URL・64 桁小文字 SHA-256・サイズ（1 B ～ 1 GiB）を明示します。空リスト時は通信不可です。
- **サンドボックス制限**：プロセス起動、任意通信、パス逸脱、シンボリックリンク、EXE の追加・変更は禁止されています。投稿チェックでは構文検証のみ行いスクリプト実行や外部アクセスは行いません。

## 3. メンテナー審査とスナップショット仕様

- **Intake レビュー**：権限を持つメンテナーが `compat:accepted` / `patch:accepted` を付与すると、自動化ワークフローが本文 SHA-256 と `updated_at` を記録したレビュー PR を生成します。承認後に本文が編集された場合は自動でラベル解除・PR クローズされます。
- **コア配布カタログ**：コアリリース時に自動で `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json` の PR が作成され、検証通過後にマージ・ポータルへ公開されます。
- **後方互換スナップショット**：`cores-v2.json` / `compatibility-v5.json` / `patches-v2.json` に加え、旧 Manager 向けに `cores.json` / `compatibility.json` / `patches.json` を併載し、`index.json` で全ファイルの SHA-256 と上限（単体 ≤ 8 MiB、合計 ≤ 64 MiB、≤ 4096 ファイル）を保証します。

```sh
cargo xtask metadata schema
cargo xtask metadata validate
cargo xtask metadata build --root Metadata --out .work/metadata-snapshot
cargo xtask metadata resolve-core --plugin-id rfvp --platform windows-x86_64 --abi shinki.emu.native.c.v3
cargo xtask metadata intake --kind compatibility --event event.json --out .work/compat-candidate
cargo xtask metadata intake --kind patch --event event.json --out .work/patch-candidate
```
