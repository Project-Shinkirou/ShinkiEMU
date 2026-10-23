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

`save_directory` 必须是游戏树外的绝对目录。纯 CP932 存档保留原生格式；使用 GBK 脚本时，采用既有 UTF-8 port 容器保存正文和注释。设备失败不悄悄切换诊断 NullAudio；原生模态请求需要可用的类型化宿主服务。

面向 eden 的实现不能推广到全部 minori 作品和发行版。它不替代[仓库内 Musica 核心](shinki-emu-musica.md)的执行器，两者配置与存档身份独立。ReMinori 保留上游 AGPL-3.0 义务。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
