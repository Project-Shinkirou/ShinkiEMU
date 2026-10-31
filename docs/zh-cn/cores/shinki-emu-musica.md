---
title: Musica
description: Musica 的格式、原生行为与限制。
---

# Musica

`plugin_id: shinki.emu.musica` · 引擎家族: Musica

这是 ShinkiEMU 仓库内的自有核心，没有独立外部 fork。实现见 [Source/Families/shinki-emu-musica](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/Families/shinki-emu-musica)，来源提交见 [ORIGINS](../../../ORIGINS.md)。

## 格式

仓库内的 Musica 核心读取 PAZ 和原生 SC，处理分卷、角色密钥、变换及有界解压。私有 `key.toml` 使用 `shinki.emu.musica.keys.v2`，算法参数和密钥都要明确提供。探测识别不会安装密钥。标准 loose SC 和配置的 PNG 替换文件限制在游戏根目录内。

## 原生行为与限制

核心 owner 运行 SC VM、硬件离屏合成和原生音频，默认先显示标题菜单，再进入剧情。输入和帧轮询不推进 VM。现有呈现包含 1280×720 布局及 Noto Sans JP 字体绑定，宿主缩放不改游戏坐标。影片 ASS 使用真实 movie 时钟和明确字体，此路径不接受 BGM 字幕映射。

未知命令、未支持演出和设备故障明确报错。音频当前采用有界完整资源解码，不是任意长媒体流式解码。

## 存档

游戏目录里有 `eden_en.exe` 或 `perseus.exe` 时，Musica 在 `<game>/.shinki/musica/minori/<title>/` 下按字节兼容的格式读写原版 minori 文件。原版把同样的文件放在 `Documents\minori\<title>\`；Musica 不会写入 Documents。eden 的标题目录名是 `eden (English edition)`，Perseus 是 `夏空のペルセウス`。原版 Perseus 用 ANSI API 处理这个名字，所以系统代码页不是 CP932 时，它在 Documents 下的目录名会显示为乱码（GBK 下为 `壞嬻偺儁儖僙僂僗`）。

| 文件 | 内容 |
| --- | --- |
| `system.dat` | 全局标志和通关状态 |
| `sysparams.txt` | 设置、快速存档和自动存档游标（Tab 分隔，CRLF） |
| `<script>.tbr` | 单个脚本的已读消息位图 |
| `cgv.dat` | 已看过的 CG 列表 |
| `edenNNNN.sav` / `edenPMNNNN.sav`、`perseus_NNNN.sav` | 原生消息检查点与历史 |
| 同名 `.png` | 不透明 RGB 区域均值缩略图：eden 128×80，Perseus 106×60 |

想沿用原版安装的进度或设置，把 `Documents\minori\<title>\` 里的文件复制到 `.shinki/musica/minori/<title>/`；要回到原版 exe 继续玩，按同样方式复制回去。`sysparams.txt` 中 Musica 用不到的键（如字体名、窗口尺寸）原样保留。原版没有对应键的设置存放在 `.shinki/musica/musica-settings.txt`，格式相同；eden 的五个角色语音开关就放在这里。

压缩文件开头有一个由 Perseus `SerialCode` 算出的前缀。在 Windows 上，Musica 读取 `HKLM\Software\minori\perseus` 的 `SerialCode` 并取 CRC32。读不到时沿用目录里已有原版文件的前缀，再没有就写 0。eden 始终用 0。

已知差异：`system.dat`、`sysparams.txt` 和文件前缀与原版逐字节一致。经 zlib 压缩的 `.tbr` 和 `cgv.dat` 解压后的内容相同，但压缩后的字节通常不同，因为 Musica 的 deflate 编码器与原版 zlib 的取舍不一样。原版 exe 可以正常读取这些文件。Musica 只在内容变化时才重写文件，所以复制过来的文件在记录新的已读标记或 CG 之前保持原样。

早期 `.shinki/musica/saves/` 中的 JSON 设置、快速存档游标和全局进度，仍按一次性、不覆盖的规则导入原生文件。无法识别的 exe 不能读写原生槽位。

运行时存读档统一使用原生后端。检查点恢复脚本变量、背景、组合前景、立绘位置与偏移、BGM、消息历史，以及小说模式（`CP1=2`）的整页文本和 `L4_n` 位置。读档会重新开始当前消息及语音；播放时钟、翻译文本和菜单焦点不写入原生槽。不支持的效果或附加图层状态会明确拒绝，不回退到内部快照。

eden 的自动槽为 0，快速槽为 1–9；`AppendPlaying` 切换到 `edenPM` 命名空间。Perseus 的快速槽为 0–9，自动槽为 10–19，手动槽从 20 开始。自动存档按原生命令计数周期触发，也在不超过四项的选择和视频开始前触发；局部 `exitFromEvent`、`prohibitAutoSave` 或全局 `prohibitSaveLoad` 非零时禁止自动存档。`prohibitSaveLoad` 同时禁止手动存读档。先写缩略图，再写压缩槽位，两者分别原子替换。

旧 beta 的 `.asav`（`AMUSSV06` 及更早格式）不兼容。Musica 不读取、转换、搬迁、改名或删除 `.shinki-musica/saves/` 和 `.shinki/musica/saves/` 中的旧文件。请保留备份，重新建立原生存档。

[ReMinori](reminori.md) 是另一个实现，执行器、配置和存档各自独立。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
