---
title: トラブルシューティングと FAQ
description: Linux ランタイム導入、Android エミュレーターの GPU 制約、DLsite 診断、セーブ移行とコア更新の解決手順。
---

# トラブルシューティングと FAQ

起動エラー、表示不具合、ストアダウンロードやセーブの問題を解決するためのチェックリストです。公式ポータルの [FAQ](https://project-shinkirou.github.io/ja/shinkiemu/faq/) もあわせてご覧ください。

## Linux デスクトップランタイム

Linux 版パッケージはディストリビューションの描画・音声・WebView ライブラリを使用します。Ubuntu 24.04 / Debian 系では以下をインストールしてください：

```sh
# Manager UI およびストア Web ログイン（WebKitGTK 4.1 + GTK 3）
sudo apt-get install -y libwebkit2gtk-4.1-0 libgtk-3-0t64 libfontconfig1 libasound2t64

# ネイティブコア向け共通ランタイム（OpenGL / X11 / Pango / OpenMP / DRM）
sudo apt-get install -y libgl1 libglx0 libopengl0 libx11-6 libgomp1 libdrm2 libpangocairo-1.0-0 libpangoft2-1.0-0

# RFVP コアの動画再生に必要な GStreamer 1.x およびプラグイン
sudo apt-get install -y libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav
```

他ディストリビューションでは相当パッケージを導入し、GPU アクセラレーション有効な X11 / Wayland 環境で起動してください。各コアの正確な SONAME 要件はコア内 `build.json` の `system_dependencies` に記録されています。

## 初回起動の同意チェックが押せない

初回起動の転売防止案内は、**案内画像全体が途切れずに 8 秒間表示**されることでチェック可能になります。ウィンドウの高さが足りない場合は縦幅を広げるかスクロールして全体を表示してください。テーマ切替を行っても累計表示時間は保持されます。

## Android エミュレーターで最初のフレームから更新されない

SwiftShader 等のソフトウェア Vulkan 描画環境では、Slint 1.18.1 が初期フレーム以降を更新しない既知の制約があります。

- **対処法**：エミュレーターを `-gpu host`（ホスト GPU 有効）で起動するか、実機 Android 端末をご利用ください。

## ストアダウンロード関連

### DLsite / DMM のログイン画面が開かない

- **Windows**：Microsoft Edge WebView2 Runtime が必要です（Windows 11 は標準搭載、Windows 10 は公式ランタイムを導入）。
- **Linux**：`libwebkit2gtk-4.1-0` をインストールしてください。

### 「Downloaded archives need manual extraction or installer run」が表示される

配布形式が RAR や自己解凍インストーラーである場合に表示されます。ShinkiEMU はダウンロードした実行ファイルを自動起動しません。「ダウンロードフォルダを開く」から手動で展開または公式インストーラーを実行し、完成したゲームフォルダをライブラリへ登録してください。

### 「Update status unavailable」と表示される

DLsite のようにローカルビルド識別子が提供されないストアやオフライン時に正常表示されます。Steam 作品はライブラリ更新で再確認できます。

### DLsite でダウンロード計画が解析できない場合の調査協力

ログイン済みブラウザで再現し、`https://play.dlsite.com/api/v3/download?workno=<作品番号>` からリダイレクトおよび `/home/download`・`/home/serial`・分割一覧までの応答付き HAR を記録できます。**共有前に Cookie・シリアル番号・署名付き URL トークンを必ず削除し、生の HAR は絶対に公開・コミットしないでください。**

## パッチ適用時にコア更新を求められる

パッチ適用や起動時に必要なコア機能不足が通知された場合（新 Musica パッチは Musica `beta.5` 以降、RFVP ネイティブスクリプトパッチはセーブ分離・設定フォント優先機能が必要）：

1. **コア**画面でカタログを更新し該当コアをアップデートします；
2. 起動中のゲームを閉じ、案内に従い Manager を再起動します；
3. パッチ適用またはゲーム起動を再試行します。

## セーブと旧ベータデータ

- **原版セーブの相互移行**：`shinki.emu.musica` ではゲーム終了状態で `Documents\minori\<title>\` と `<game>/.shinki/musica/minori/<title>/` のファイルを相互コピーできます。
- **旧ベータの `.asav` ファイル**：初期 Musica ベータの `.asav`（`AMUSSV06` 以前）は非互換です。ShinkiEMU は `.shinki-musica/saves/` 等の旧ファイルを削除・変更しませんので、バックアップのうえ原版形式セーブをご利用ください。
- **`Manager 0.1.0-beta.7` Windows 版のバージョン表示**：公式 `beta.7` Windows バイナリが内部で `0.1.0-beta.6` と表示する既知問題がありました。`0.1.0-beta.8` で修正済みです。

## 解決しない場合

- **質問・相談**：[GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions) へ投稿してください；
- **再現可能な不具合**：[バグ報告フォーム](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)からご報告ください；
- **外部エンジン本体に起因する問題**：各コアページ記載のアップストリームリポジトリもあわせてご参照ください。
