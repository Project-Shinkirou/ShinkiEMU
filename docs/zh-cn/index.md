---
title: ShinkiEMU 文档
description: 面向玩家、社区贡献者与开发者的 ShinkiEMU 完整文档导航。
---

# ShinkiEMU 文档

ShinkiEMU 由跨平台游戏库管理器 **Manager** 与模块化**原生引擎核心**组成，用于让视觉小说在现代电脑与手机上原生运行。Manager 负责游戏库管理、核心安装、元数据同步、商店下载与补丁运行；每个核心独立保留对应引擎的虚拟机、渲染、音频、游戏菜单与存档逻辑。底层共享库来自姊妹创作引擎 [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine)。

## 新玩家从这里开始

- [快速上手](quick-start.md)：3 分钟完成下载、添加游戏、安装核心并启动游玩。
- [用户指南](user-guide.md)：界面语言、更新检查、核心管理、Steam/DLsite 商店下载、Luau 补丁与原版存档互通。
- [故障排查](troubleshooting.md)：Linux 桌面运行库、Android GPU 说明、DLsite 下载诊断与常见问题处理。

## 参与社区与贡献

- [贡献指南](contributing.md)：无需写代码的参与路径、架构速览、界面翻译（`.po`）、AI 辅助开发规范与 PR 流程。
- [元数据与补丁贡献](metadata.md)：提交游戏兼容性报告与 Luau 补丁配方，以及快照审查规则。
- [社区讨论（GitHub Discussions）](https://github.com/Project-Shinkirou/ShinkiEMU/discussions)：提问求助、分享体验与讨论新想法。

## 开发者与维护者

- [从源码构建](build-and-release.md)：工具链准备、`cargo xtask` 常用命令与各平台本地构建入口。
- [核心 ABI 与 SDK](core-abi.md)：C ABI v3 生命周期、能力位、异步文本替换与 CPU 帧租约契约。
- [无窗口运行与界面截图](headless.md)：`shinki-emu-headless` 出帧验收、输入回放与 Manager 深浅主题离屏截图。
- [发布与维护手册](release-runbook.md)：依赖锁定清单、CI 矩阵、Apple/OHOS 发布交接、源码与许可打包规范及版本交付记录。

## 核心概念与各核心资料

在 ShinkiEMU 中，**引擎家族（family）**代表原引擎或格式体系，**核心（core）**是该引擎的一种具体实现，**`plugin_id`** 是安装与元数据使用的区分大小写精确标识（例如 `shinki.emu.musica` 与 `ReMinori` 同属 Musica 家族，但执行器、配置与存档相互独立）。

[RFVP](cores/rfvp.md) · [Siglus](cores/siglus-rs.md) · [Artemis](cores/art3m1s-core.md) · [Musica（自研）](cores/shinki-emu-musica.md) · [ReMinori](cores/reminori.md) · [CMVS（自研）](cores/shinki-emu-cmvs.md) · [KiriKiri2](cores/kirikiroid2-web.md) · [impacto](cores/impacto.md) · [Ren’Py](cores/renpy.md) · [Sena](cores/sena-rs.md) · [Ethornell](cores/ethornell-rs.md)

实时发布版本见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，各作品实测状态见[兼容性列表](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。
