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

产品自有槽位于 `.shinki-musica/saves/`，绑定 archive/profile/localization/font 身份。无效候选不改变活动状态或覆盖现有文件。原版 eden 存档互通仅限明确支持的消息检查点，不代表通用原版存档兼容。未知命令、未支持演出和设备故障明确报错。音频当前采用有界完整资源解码，不是任意长媒体流式解码。

[ReMinori](reminori.md) 是另一个实现，执行器、配置和存档各自独立。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
