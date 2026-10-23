---
title: Sena
description: Sena 的格式、原生行为与限制。
---

# Sena

`plugin_id: sena-rs` · 引擎家族: SoftPAL

上游：[xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs)。适配 fork：[Project-Shinkirou/sena-rs 的 shinki 分支](https://github.com/Project-Shinkirou/sena-rs/tree/shinki)。

## 格式

SoftPAL 将 `SCRIPT.SRC` 字节码与 `POINT.DAT` 分支/调用目标、`FILE.DAT` 资源名、`TEXT.DAT` 文本及 `MEM.DAT` 初始数据一起解释。`Sv20` 头长 12 字节，包含明确入口 PC。指令与数据采用 little-endian word；extcall 按原生弹栈顺序取参，不能按文档展示顺序读取。

## 原生行为与限制

fork 拥有 SoftPAL VM、等待语义、原生渲染/音频和存档。原生读档成功后应继续允许保存。适配层提供 `text_language=jp/cn/tc`，多语言字段选择保留原日文单字段行为。字体来自原生游戏路径或显式原生配置，渲染尺寸通过 ABI v3 请求。

适配范围仍是原生及 Steam Koikake 路径，不能推广到所有 SoftPAL 作品。未知 extcall 和间接内存路径需要按实际运行时确认，识别出数据表还不够。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
