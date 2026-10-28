---
title: ReMinori
description: ReMinori 的格式、原生行为与限制。
---

# ReMinori

`plugin_id: ReMinori` · 引擎家族: Musica

上游：[luoyily/ReMinori](https://github.com/luoyily/ReMinori)。适配 fork：[Project-Shinkirou/ReMinori 的 shinki 分支](https://github.com/Project-Shinkirou/ReMinori/tree/shinki)。

## 格式

ReMinori 是独立的 Musica 实现。它的原生 PAZ reader 处理 0/1/2 版本、按角色配置的密钥、变换和 zlib 条目。可用私有 `shinki.emu.musica.keys.v2` profile 指定归档参数；未指定时沿用上游零售 CD-ROM 参数。脚本严格比较 CP932/GBK 的有效性，剩余非法字节明确拒绝。

## 原生行为与限制

适配保留原 VM/IR、GPU 合成、CPAL mixer、Media Foundation 视频、菜单和存档实现。交付原生 1024×640、top-down BGRA8 CPU 租约，不提供可调渲染尺寸。三个复用 CPU 槽保护借出的帧，原生执行可继续进行。

嵌入 ShinkiEMU 时，ReMinori 使用 `<game>/userdata/`；上游独立版使用可执行文件同目录的 `userdata/`。该目录无法创建或写入时，会按上游行为回退到 `Documents/eden/userdata/`。ReMinori 在此保存 `edenNNNN.sav` 及其缩略图 `edenNNNN.png`、快速存档 `edenPMNNNN.sav`、加密压缩的进度文件 `system.dat`、`settings.tsv`、`readflags.bin` 和 `cgseen.tsv`。解码后的影片沿用上游缓存 `<系统缓存目录>/eden/movies/`。

纯 CP932 存档保留原生格式。使用 GBK 脚本时，ReMinori 用 UTF-8 port 存档容器保存正文和注释，这类存档不保证能被原版 eden 读取。音频设备失败时不会悄悄换成诊断用的 NullAudio；原生模态窗口需要可用的类型化宿主服务。

原版 Windows eden 把存档放在 `Documents\minori\<作品名>\`，英文版是 `Documents\minori\eden (English edition)\`。要在 ReMinori 中使用，请把 `edenNNNN.sav`、`edenNNNN.png` 和 `system.dat` 复制到当前使用的 `userdata/` 目录；要把兼容的零售版格式存档交还原版 eden，反向复制即可。请先备份两个目录，复制时不要覆盖已有文件。

如果在 beta.1/beta.2 中手动选择过 `save_directory`，请将其中的文件手动复制到当前使用的 `userdata/` 目录，不要覆盖已有文件。

面向 eden 的实现不能推广到全部 minori 作品和发行版。它不替代[仓库内 Musica 核心](shinki-emu-musica.md)的执行器，两者配置与存档身份独立。ReMinori 保留上游 AGPL-3.0 义务。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
