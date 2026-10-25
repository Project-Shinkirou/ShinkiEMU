---
title: ShinkiEMU
description: 旧世代のビジュアルノベルエンジン向けの Manager とネイティブコア。
---

# ShinkiEMU

ShinkiEMU は、ゲームライブラリを管理する Manager とネイティブコアで構成され、旧世代のビジュアルノベルエンジンで作られたゲームを動かします。Manager はゲームの検出、コアのインストール、コア画面の表示、メタデータとの連携を担当します。VM、描画、音声、ゲーム内メニュー、セーブの挙動は各コアが担当します。[RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) は独立した制作用エンジンで、共通の低レベルライブラリも提供しています。

## コア、エンジンファミリー、識別子

- **family** は元のエンジンまたはフォーマットのファミリーを表します。
- **core（コア）** はそのエンジンの実装の一つです。
- **plugin_id** はインストールとメタデータで使う正確な識別子で、大文字と小文字を区別します。外部コアは上流リポジトリ名をそのまま使います（例: `ReMinori`）。

`shinki.emu.musica` と `ReMinori` はどちらも Musica ファミリーに属しますが、別々のコアです。実行部、設定、セーブの識別はそれぞれ独立しています。一方をインストール・設定しても、もう一方に同じ操作が行われることはありません。

## 利用と開発

- [ユーザーガイド](user-guide.md)：インストール、ゲームの追加、コアの更新、パッチの利用。
- [ビルドとリリース](build-and-release.md)：Cargo のエントリーポイントと blocked ターゲット。
- [コア ABI と SDK](core-abi.md)：ネイティブの所有権とフレームのライフサイクル。
- [Metadata への貢献](metadata.md)：互換性レポート、パッチ、レビューの流れ。

## 各コアの資料

[RFVP](cores/rfvp.md) · [Siglus](cores/siglus-rs.md) · [Artemis](cores/art3m1s-core.md) · [Musica](cores/shinki-emu-musica.md) · [Sena](cores/sena-rs.md) · [KiriKiri](cores/kirikiroid2-web.md) · [Ethornell](cores/ethornell-rs.md) · [CMVS](cores/shinki-emu-cmvs.md) · [ReMinori](cores/reminori.md) · [impacto](cores/impacto.md) · [Ren’Py](cores/renpy.md)。

現在のビルドとリリースの情報は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、ゲームの動作結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)で確認できます。ビルドの成功やプローブの一致は、ゲームがプレイできることを意味しません。該当するレポートがない場合は「報告なし」と表示されます。
