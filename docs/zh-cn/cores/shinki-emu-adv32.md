---
title: Shinki ADV32
description: CIRCUS ADV32 核心的开发状态和本地运行方式。
---

# Shinki ADV32

`shinki.emu.adv32` 是 CIRCUS ADV32 引擎的桌面开发预览。核心从游戏目录的 `AdvData` 中读取 MES 指令、CRX 图像、CRM/PCK 资源包和原生音频，通过 C ABI v3 提供 CPU 帧。目前没有公开下载，也未加入移动端 Manager。

预览版实现了部分剧情、选项、菜单、影片和原生存档功能。发布前还需验证完整的 VM 指令覆盖和原版存档互通。请使用自己合法取得的游戏文件，并保留原版可执行文件。核心会从中读取存档初始设置、存档文件名前缀、音乐列表和已支持的鉴赏数据。核心不附带原版游戏文件，遇到未支持的指令会报告脚本名和字节位置。

默认使用 CPU 合成。开启**硬件合成**（`hardware_compositing`）后，由 GPU 合成图层和文字，持续动画期间最多有一帧正在合成。C ABI v3 仍需把帧回读到 CPU 内存，影片解码和逐像素遮罩也仍由 CPU 处理。暂无实测提速数据。将 GPU 合成与回读合并提交的共享渲染器改动未纳入本预览。

用独立 headless 程序加载桌面核心。

```sh
cargo build -p shinki-emu-adv32 --features dynamic-plugin-export,video-ffmpeg
cargo build -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

带原生存档的测试可用 `--copy-out AdvData/SAVEDATA` 导出运行副本中的存档目录。导出只在会话成功关闭后执行，结果在输出目录的 `game-copy-out` 下。
