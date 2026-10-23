---
title: Siglus
description: Siglus 的格式、原生行为与限制。
---

# Siglus

`plugin_id: siglus_rs` · 引擎家族: SiglusEngine

上游：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)。适配 fork：[Project-Shinkirou/siglus_rs 的 shinki 分支](https://github.com/Project-Shinkirou/siglus_rs/tree/shinki)。

## 格式

`Scene.pck` 汇集 scene chunk 及包级 include 属性/命令。包偏移相对包起点，解码后 `.ss` 中的偏移相对 chunk 起点。字符串索引的偏移和长度按 UTF-16 code unit 计数，不是字节；存储字串带与索引有关的逐 word 变换。scene、label 和 command entry 表各自参与 VM 控制流。

## 原生行为与限制

fork 保留 Siglus VM、renderer、文字/键盘输入和原生存档。适配层拥有执行循环并发布 CPU 帧。原生配置支持 original/chs 成对资源、G01/movie 选择和独立保存根。

本地化发行版的 wrapper 不属于嵌入运行时。核心不应用 wrapper 机器码补丁，也不补造缺失字体；包解码、资源配置和运行行为都要按具体发行版确认。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
