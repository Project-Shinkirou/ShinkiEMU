---
title: Ethornell / BGI
description: Ethornell / BGI 的格式、原生行为与限制。
---

# Ethornell / BGI

`plugin_id: ethornell-rs` · 引擎家族: Ethornell / BGI

上游：[xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs)。适配 fork：[Project-Shinkirou/ethornell-rs 的 shinki 分支](https://github.com/Project-Shinkirou/ethornell-rs/tree/shinki)。

## 格式

BGI 脚本按解码后的字节识别，不能只看扩展名。归档条目外层可能是 `DSC FORMAT 1.00`；BCS 以 `BurikoCompiledScriptVer1.00` 开头，BP 系统程序常用 `._bp`。无头剧情是另一种变体。BCS 的 body-relative 地址和 BP 的文件/代码偏移不能混用。

## 原生行为与限制

fork 拥有 VM dispatch、图形、文字、音频和存档状态，原生适配层发布最终 CPU 帧。system、graph、sound、user 分派组保留原生参数栈语义。CP932 与本地化编码需要按实际发行版配置，不能统一猜测转换。

识别归档/脚本头不代表每个 dispatch 或早期变体都已实现。错误报告只保留命令编号、位置和复现步骤，不转储商业字符串。分发保留 fork 中的 MPL-2.0 全文和修改归属。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
