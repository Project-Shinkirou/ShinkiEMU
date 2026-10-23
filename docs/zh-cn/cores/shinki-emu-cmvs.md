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

同一时间只允许一个 CMVS 会话。当前 reader 拒绝未支持 CPZ 版本、越界路径、未知命令及未恢复字符串域。字体要求精确匹配原生字体，不以通用字体替代；Documents/AppData 保存根行为尚未恢复。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
