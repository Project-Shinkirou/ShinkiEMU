---
title: RFVP
description: RFVP 的格式、原生行为与限制。
---

# RFVP

`plugin_id: rfvp` · 引擎家族: FVP

上游：[xmoezzz/rfvp](https://github.com/xmoezzz/rfvp)。适配 fork：[Project-Shinkirou/rfvp 的 shinki 分支](https://github.com/Project-Shinkirou/rfvp/tree/shinki)。

## 格式

FVP 使用 HCB 字节码。文件首个 little-endian u32 是系统描述区偏移，代码从第 4 字节开始，不靠 magic 字符串识别。描述区包含入口、全局变量和 syscall 表。带长度的 C 字符串把末尾 NUL 计入长度；编码由核心配置明确选择。

## 原生行为与限制

fork 保留原 App、VM、GPU renderer、设备音频和存档。适配层用不可变 CPU 租约交付最终场景，Manager 不重写 syscall，也不通过宿主 tick 推进。原生配置可选择按原 HCB 偏移索引的 UTF-8 翻译表，以及使用明确字体和原媒体时钟的 movie/BGM ASS 映射。

WMV/ASF/WMA 走 fork 原生影片及 decoder 路径。原生请求的字体和游戏资源仍是实际输入；识别出 HCB 不代表 syscall 已全部实现或所有发行版可用。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
