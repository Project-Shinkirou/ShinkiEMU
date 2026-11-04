---
title: KrKr2
description: モバイル Manager 向けの独立したネイティブ KrKr2 コア。
---

# KrKr2

`plugin_id: krkr2` · エンジン系統：KiriKiri

上流：[2468785842/krkr2](https://github.com/2468785842/krkr2)。アダプター：[Project-Shinkirou/krkr2, shinki](https://github.com/Project-Shinkirou/krkr2/tree/shinki)。

## 形式と選択

`startup.tjs` の信頼度は 9400、`data.xp3` は 9300 です。既存の `kirikiroid2-web` は別 ID のままで、両方が一致した場合は従来どおり 9500 で優先されます。この実装を使う場合は `krkr2` を選択してください。形式の検出はゲームの互換性を保証しません。

## ネイティブ動作

上流独自のモジュール化 C++ TJS VM、KAG、描画、プラグイン、フォント、セーブと設定を使用します。kirikiroid2-web への転送は行いません。Android/iOS のネイティブ静的シンボルを分離し、Android の Java/JNI パッケージも独立させています。元のゲームテクスチャを不変 CPU フレームとして公開し、入力と一時停止/再開はネイティブ owner スレッドに渡します。キャンセルと終了時にはワーカーを join します。

このブランチは Android/iOS Manager に統合します。単体コア配布は宣言していません。構築には対象 SDK、Bison、コア専用 vcpkg 依存関係が必要です。リリース受け入れと実ゲームの互換性は別途記録します。ライセンスと変更内容は fork の `LICENSE` と `MODIFICATIONS.md` にあります。

Linux のビルドホストには Yasm も必要です（Ubuntu では `apt-get install yasm`）。固定した FFmpeg の x86/x64 ポートは依存関係の準備中にこのツールを検出します。

macOS で x86/x64 向けにビルドする場合も、`brew install yasm` が必要です。
