---
title: Ren’Py
description: Ren’Py 的格式、原生行为与限制。
---

# Ren’Py

`plugin_id: renpy` · 引擎家族: Ren’Py

上游：[renpy/renpy](https://github.com/renpy/renpy)。适配 fork：[Project-Shinkirou/renpy 的 shinki 分支](https://github.com/Project-Shinkirou/renpy/tree/shinki)。

## 格式与执行

该核心嵌入官方 Ren’Py 运行时，不把脚本转成新的 VM。脚本执行、Screen Language、ATL、GL2 合成、媒体和存档沿用官方行为。接入固定官方 SDK、CPython 3.12.7、Cython 3.2.4 和 SDL 2.0.20，DLL 校验和及来源记录在 `shinki/sdk.json`。

## 存储

适配层保留 Ren’Py 8.5.3 的存档路径行为。设置 `config.save_directory` 时，运行时会同步游戏目录中的 `<game>/game/saves` 与 `%APPDATA%/RenPy/<config.save_directory>`；未设置时使用 `<game>/game/saves`。存档令牌和备份也沿用上游解析规则，不需要 Shinki 专用数据目录。

## 原生行为与限制

运行时桥保留在驻留解释器中。原生构建产出 display 扩展和真实共享核心，包内还需匹配的 SDK DLL 与 `Resources/`。关闭和跨游戏重开继续清理解释器/原生状态，不能只复制核心 DLL。

现有嵌入要求 Windows x64/MSVC。上游 Ren’Py 支持某个平台，不代表此适配已经移植。官方标准归档 reader 保持不变，发行版专用离线转换由已审查配方处理。不打包 Steam、Cubism、Bink、nvdrs 或可再分发 D3DCompiler DLL。

运行时包含多种许可，其中 LGPL 依赖保留对应源码和重链接义务。完整保留官方声明，不能把整个运行时标成 MIT。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
