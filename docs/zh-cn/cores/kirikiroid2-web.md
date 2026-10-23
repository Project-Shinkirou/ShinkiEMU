---
title: KiriKiri / Kirikiroid2-web
description: KiriKiri / Kirikiroid2-web 的格式、原生行为与限制。
---

# KiriKiri / Kirikiroid2-web

`plugin_id: kirikiroid2-web` · 引擎家族: KiriKiri

上游：[fenghengzhi/kirikiroid2-web](https://github.com/fenghengzhi/kirikiroid2-web)。适配 fork：[Project-Shinkirou/kirikiroid2-web 的 shinki 分支](https://github.com/Project-Shinkirou/kirikiroid2-web/tree/shinki)。

## 格式

KiriKiri storage 组合 XP3、散文件和原生 KAG/TJS 资源，保留的原生引擎也处理其 PSB/KBAD 路径。挂载顺序遵守原生启动/挂载规则，不能用文件名字母序替代。带 `TJS2100` 标识的补丁脚本是字节码，不是 XP3 或普通文本。

## 原生行为与限制

C++ 核心直接导出统一 ABI，保留原生 VM、渲染、字体、菜单和存档。Cargo query binding 与构建脚本驱动真实原生共享目标。WindowEx overlay 所有权和可重入清理仍由原生生命周期管理，不使用子进程/IPC 游戏宿主。

完整包需要真实原生依赖和所需资源，不能只交付 Rust binding 库。FFmpeg 3.3.9 动态链接，仍须满足对应源码和重链接义务。能加载 XP3 不代表第三方原生插件或特定发行版归档过滤器已经兼容。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
