---
title: CMVS
description: CMVS 的格式、原生行为与限制。
---

# CMVS

`plugin_id: shinki.emu.cmvs` · 引擎家族: CMVS

这是 ShinkiEMU 仓库内的自有核心，没有独立外部 fork。实现见 [Source/Families/shinki-emu-cmvs](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/Families/shinki-emu-cmvs)，来源提交见 [ORIGINS](../../../ORIGINS.md)。

## 格式

仓库内的 CMVS 核心读取 CPZ5，并由原生调度器执行 PS2A。`cmvs.profile.json` 使用 `shinki.emu.cmvs.profile.v2`，有序 archive 角色和按角色选择的 loose 文件都需明确配置。默认入口为 `start.ps3`，Script slot 0 从 `cmvs.cfg` 初始化。脚本、纹理和原生搜索域保持独立；选中的 loose 资源损坏时，不悄悄换成归档内同名文件。

## 原生行为与限制

原生 owner 管理 VM、Scene 和媒体，保留 PS2A 返回码弹栈及原生 693/694 存读档。进入菜单时悬挂剧情上下文；恢复保留槽对象、纹理关系和摆放，不把菜单临时调度状态当成剧情。损坏存档候选不能部分替换活动状态或覆盖原文件。

核心自有音频混合 BGM、crossfade 和音效，并转换到真实设备格式。MGV 保留 slot-2 音频时钟及既有 Theora 规则。取消唤醒队列，关闭等待 VM、音频和 decoder 退出，CPU 租约仍可在关闭后归还。

同一时间只允许一个 CMVS 会话。当前 reader 拒绝未支持 CPZ 版本、越界路径、未知命令及未恢复字符串域。所有平台用同一个可移植光栅器按 GDI 带 hinting 的 17 级 `GGO_GRAY4_BITMAP` 规则生成字形，字宽与 GDI 一致；灰度位图接近但不完全相同：MS Gothic 12–48 px 的全部 7494 个 CP932 字符中，着墨像素平均相差 0.05–0.43 级，外框一致的字形占 64–88%。未安装的字体族改用核心内置的 Noto Sans JP，相当于 GDI 字体映射替换另一款字体。原引擎从 `HKCU\Software\<REG>` 读取的注册表值改从游戏目录 `.shinki/cmvs/registry.json` 读取。存档保留原引擎的 `game/save/` 位置和格式；Documents/AppData 保存根行为尚未恢复。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
