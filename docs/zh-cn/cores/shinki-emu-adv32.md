---
title: Shinki ADV32
description: CIRCUS ADV32 核心的开发状态和本地运行方式。
---

# Shinki ADV32

`shinki.emu.adv32` 是 CIRCUS ADV32 引擎的核心。它从游戏目录的 `AdvData` 中读取 MES 指令、CRX 图像、CRM/PCK 资源包和原生音频，通过 C ABI v3 提供 CPU 帧。桌面端可单独构建动态库。

核心通过 `AdvData/MES/start.mes` 识别 MES 方言，接受 `0x9e97`、`0x9f9a`、`0xa6b9`、`0xaab6` 和 `0xabb6`。能识别目录不代表支持该游戏或发行版的全部剧情和菜单。请保留原版可执行文件，核心会尝试从中读取存档初始设置、存档文件名前缀、音乐列表和已支持的鉴赏数据。核心不附带原版游戏文件；遇到未支持的指令时会报告脚本名和字节位置。

默认使用 CPU 合成。在 Windows、Linux、macOS、Android 和 iOS 上，开启 `hardware_compositing` 后可由 GPU 合成图层和文字，持续动画期间最多有一帧正在合成。C ABI v3 仍需把帧回读到 CPU 内存，影片解码和逐像素遮罩也由 CPU 处理。OpenHarmony 使用 CPU 合成和 OHAudio。

如果系统已安装原版可执行文件指定的字体，核心会使用该字体。也可以用 `font_file` 指定游戏目录中的字体，例如翻译补丁附带的字体。

脚本请求启动授权时，Windows 核心会按照原版可执行文件中的卷标检查已挂载的光盘。不支持基于注册表的免盘授权。若要从已安装的文件启动并跳过光盘检查，请显式开启 `skip_disc_check`（默认关闭）。用 headless 测试时，将 `[{"id":"skip_disc_check","value":{"Bool":true}}]` 写入 `--configuration` 指定的文件。

ADV32 作为独立动态核心安装。发版前按游戏发行版验证 VM 和原版存档互通，并核对[对应源码、许可证和 DLL 替换步骤](../../licenses/shinki-emu-adv32/WINDOWS-FFMPEG.md)。

Windows 验收候选是 Dal Segno（MES `0x9f9a`，存档前缀 `DS`）。这一格式的 `AdvData/SAVEDATA/system.dat` 为 19,736 字节，槽位文件为 72,440 字节。独立核心还没有通过验收的游戏版本。交换存档前请备份，并在同一版本上分别验证原版和核心能否载入对方写出的存档。

用独立 headless 程序加载桌面核心。

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

带原生存档的测试可用 `--copy-out AdvData/SAVEDATA` 导出运行副本中的存档目录。导出只在会话成功关闭后执行，结果在输出目录的 `game-copy-out` 下。
