---
title: Artemis / Art3m1s
description: Artemis / Art3m1s 的格式、原生行为与限制。
---

# Artemis / Art3m1s

`plugin_id: art3m1s-core` · 引擎家族: Artemis

上游：[Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)。适配 fork：[Project-Shinkirou/art3m1s-core 的 shinki 分支](https://github.com/Project-Shinkirou/art3m1s-core/tree/shinki)。

## 格式

Artemis 读取 `system.ini`、PFS（PF6/PF8）及原生剧情/tag/Lua 资源。标准数字补丁卷 `.pfs.000` 至 `.pfs.999` 覆盖基包，活动卷中高编号优先。备份后缀不会自动作为补丁挂载；保留原生散文件与包的优先级。

## 原生行为与限制

核心保留 Lua 5.1、原生图形路径和 FFmpeg。它的 Lua 依赖图与 Manager 的 Luau 配方分开。适配层拥有调度和设备音频，用有界不可变 CPU 槽交付帧。原生模态对话框走类型化宿主服务，退出只结束会话，不结束 Manager 进程。

`extra_archive` 显式选择相对路径 PFS，挂在正常数字补丁链之后；不扫描备份、不重写基包。descriptor 不提供可调渲染尺寸。缺脚本目标、缺资源或未支持的原生 UI/媒体命令仍报错；能点击标题界面不代表完整游戏流程可用。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
