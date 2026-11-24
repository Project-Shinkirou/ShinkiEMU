---
title: Shinki ADV32
description: CIRCUS ADV32 核心的开发状态和本地运行方式。
---

# Shinki ADV32

`shinki.emu.adv32` 是 CIRCUS ADV32 引擎的核心。它从游戏目录的 `AdvData` 中读取 MES 指令、CRX 图像、CRM/PCK 资源包和原生音频，通过 C ABI v3 提供 CPU 帧。桌面端可单独构建动态库。

核心可运行剧情、选项、菜单和影片，并读写已实现格式的原版存档。请在游戏目录内保留原版可执行文件，核心会从中读取存档初始设置、存档文件名前缀、音乐列表和已支持的鉴赏数据。核心不附带原版游戏文件；遇到未支持的指令时会报告脚本名和字节位置。

默认使用 CPU 合成。在 Windows、Linux、macOS、Android 和 iOS 上，开启 `hardware_compositing` 后可由 GPU 合成图层和文字，持续动画期间最多有一帧正在合成。C ABI v3 仍需把帧回读到 CPU 内存，影片解码和逐像素遮罩也由 CPU 处理。OpenHarmony 使用 CPU 合成和 OHAudio。

如果系统已安装原版可执行文件指定的字体，核心会使用该字体。也可以用 `font_file` 指定游戏目录中的字体，例如翻译补丁附带的字体。

脚本请求启动授权时，Windows 核心会按照原版可执行文件中的卷标检查已挂载的光盘。不支持基于注册表的免盘授权。若要从已安装的文件启动并跳过光盘检查，请显式开启 `skip_disc_check`（默认关闭）。用 headless 测试时，将 `[{"id":"skip_disc_check","value":{"Bool":true}}]` 写入 `--configuration` 指定的文件。

桌面端、Android、iOS 和 OpenHarmony 的本地 Manager 构建可通过 `--features static-adv32` 静态接入。以上是接入目标，不代表构建或游玩已通过验收。公开版 Manager 不内置 ADV32，独立核心也要在平台和游玩验收通过后才发布。

用独立 headless 程序加载桌面核心。

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

带原生存档的测试可用 `--copy-out AdvData/SAVEDATA` 导出运行副本中的存档目录。导出只在会话成功关闭后执行，结果在输出目录的 `game-copy-out` 下。
