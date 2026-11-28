---
title: Artemis / Art3m1s
description: Artemis / Art3m1s 的格式、原生行为与限制。
---

# Artemis / Art3m1s

`plugin_id: art3m1s-core` · 引擎家族: Artemis

发布身份跟随上游发布标签 [v0.4.0-fix.1](https://github.com/Alphaly2K/art3m1s-core/releases/tag/v0.4.0-fix.1)。`Cargo.lock` 固定 [ShinkiEMU 适配提交](https://github.com/Project-Shinkirou/art3m1s-core/tree/a761f0926ee3c7280d696c8c36763500cd319194)，仍保留标签之后的上游修复，包括指针拖拽处理。

## 格式

Artemis 读取 `system.ini`、PFS（PF6/PF8）及原生剧情/tag/Lua 资源。标准数字补丁卷 `.pfs.000` 至 `.pfs.999` 覆盖基包，活动卷中高编号优先。备份后缀不会自动作为补丁挂载；保留原生散文件与包的优先级。

## 原生行为与限制

核心在各平台使用 Luau，由 Artemis 兼容层提供旧 Lua API，并保留原生图形路径和 FFmpeg。Android 独立核心在激活时绑定应用上下文。适配层负责调度和设备音频，用有界的只读 CPU 缓冲区交付画面。原生模态对话框通过宿主服务显示，退出游戏只关闭会话。

`extra_archive` 显式选择相对路径 PFS，挂在正常数字补丁链之后；不扫描备份、不重写基包。descriptor 不提供可调渲染尺寸。缺脚本目标、缺资源或未支持的原生 UI/媒体命令仍报错；能点击标题界面不代表完整游戏流程可用。

## 存档路径

适配层的可写根目录为 `<游戏>/savedata/`。`system.ini` 中的 `SAVEPATH` 指定其下的相对目录，缺失或归一化后为空时用 `save`。`save0001.dat` 等槽位、`saveg.dat` 和 `system.dat` 默认位于 `<游戏>/savedata/save/`；设为 `SAVEPATH=savedata` 时位于 `<游戏>/savedata/savedata/`。绝对路径或 CSIDL 风格的值会归一化成逻辑前缀，不会直接用作系统路径。已有存档路径不变。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
