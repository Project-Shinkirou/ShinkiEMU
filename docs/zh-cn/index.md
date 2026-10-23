---
title: ShinkiEMU
description: 面向旧视觉小说引擎的 Manager 与原生核心。
---

# ShinkiEMU

ShinkiEMU 由游戏库管理器 Manager 和原生核心组成，用来运行旧视觉小说引擎的游戏。Manager 负责发现游戏、安装核心、显示核心画面和接入元数据；VM、渲染、音频、游戏菜单及存档行为由各核心负责。[RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) 是独立的创作引擎，也提供共用底层库。

## 核心、引擎家族与标识

- **family** 表示原引擎或格式家族。
- **core（核心）** 是该引擎的一种实现。
- **plugin_id** 是安装和元数据使用的精确标识，区分大小写。外部核心沿用上游仓库名，例如 `ReMinori`。

`shinki.emu.musica` 与 `ReMinori` 都属于 Musica 家族，但它们是两个核心，执行器、配置和存档身份各自独立。安装或配置其中一个，不会替另一个完成相同操作。

## 使用与开发

- [用户指南](user-guide.md)：安装、添加游戏、更新核心与使用补丁。
- [构建与发布](build-and-release.md)：Cargo 入口与 blocked 目标。
- [核心 ABI 与 SDK](core-abi.md)：原生所有权和帧生命周期。
- [Metadata 贡献](metadata.md)：兼容性报告、补丁和审查流程。

## 各核心资料

[RFVP](cores/rfvp.md) · [Siglus](cores/siglus-rs.md) · [Artemis](cores/art3m1s-core.md) · [Musica](cores/shinki-emu-musica.md) · [Sena](cores/sena-rs.md) · [KiriKiri](cores/kirikiroid2-web.md) · [Ethornell](cores/ethornell-rs.md) · [CMVS](cores/shinki-emu-cmvs.md) · [ReMinori](cores/reminori.md) · [impacto](cores/impacto.md) · [Ren’Py](cores/renpy.md)。

当前构建和发布信息见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，游戏运行结论见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。编译成功或探测命中不代表游戏可玩；没有对应报告时显示“尚无报告”。
