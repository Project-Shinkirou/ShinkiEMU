---
title: impacto
description: impacto 的格式、原生行为与限制。
---

# impacto

`plugin_id: impacto` · 引擎家族: MAGES.

上游：[CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto)。适配 fork：[Project-Shinkirou/impacto 的 shinki 分支](https://github.com/Project-Shinkirou/impacto/tree/shinki)。

## 格式与执行

impacto 保留 MAGES SC3 VM、原生资源 reader 和游戏专用配置。C++ 接入仍位于 `src/shinki`，Cargo/CMake 构建入口位于 `shinki`。OpenGL 合成、OpenAL 音频、FFmpeg 影片、游戏 UI 和存档继续由原生引擎执行。

## 原生行为与限制

适配层提供 C ABI v3 和 CPU 帧所有权，不在 Manager 重写 MAGES 脚本。fork 保留 DDS 并发读取状态与影片音频释放的所有权修复。Steam 字库从用户实际游戏的 DDS 读取，不作为替代资源分发。

开源依赖路径不打包或加载 Bink DLL。游戏系统数据错误和未运行路线仍须分别记录，query 成功不能证明系统页、影片或存档可用。整体 ISC 与各组件许可同时保留，二进制分发需要核齐原生依赖及对应源码。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
