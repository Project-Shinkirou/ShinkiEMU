---
title: Siglus
description: Siglus 的格式、原生行为与限制。
---

# Siglus

`plugin_id: siglus_rs` · 引擎家族: SiglusEngine

上游：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)。`Cargo.lock` 固定了 [ShinkiEMU 适配源码](https://github.com/Project-Shinkirou/siglus_rs/tree/575075a678a17d4e21ac81c24adbef124ab2027e)。

## 格式

`Scene.pck` 汇集 scene chunk 及包级 include 属性/命令。包偏移相对包起点，解码后 `.ss` 中的偏移相对 chunk 起点。字符串索引的偏移和长度按 UTF-16 code unit 计数，不是字节；存储字串带与索引有关的逐 word 变换。scene、label 和 command entry 表各自参与 VM 控制流。

## 原生行为与限制

fork 保留 Siglus VM、renderer、文字/键盘输入和原生存档。适配层拥有执行循环并发布 CPU 帧。原生配置支持 original/chs 成对资源和 G01/movie 选择。

场景重启后保留会话的图片和影片资源语言。CHS 模式选择原生 G01 和 `mov_chs` 替换资源，不覆盖原版 G00 文件。

Android Manager 在启动游戏时从独立核心包加载 Siglus。扫描只读核心包的元数据，不加载库。激活时 Manager 通过 ABI 扩展传入 Android 上下文；Siglus 仍提供 CPU 帧。

Android 独立核心的 Oboe 音频使用 NDK 的 `libc++_shared.so`，打包时会附带对应 ABI 的运行库及 NDK 许可声明。

## 存档与密钥

核心把存档写入独立目录。Windows 默认从 `%LOCALAPPDATA%` 开始，macOS 用 `~/Library/Application Support`，Linux 用 `$XDG_DATA_HOME` 或 `~/.local/share`，Android 用应用私有文件目录。核心在对应目录后追加 `siglus_rs/saves/<规范化游戏路径的哈希>/`。

显式设置 `save_root` 会替代这整段路径，须为与游戏目录和原版存档目录相互独立的绝对路径。不论是否显式设置，核心都会再追加 `siglus-<original|chs>-<资源对标识>/`。存档和原生设置写进其中的 `savedata/`，截图和导出也相对同一目录写入。原版和 CHS 资源不自动共用存档。

核心不会导入或重命名旧档。若要尝试其他路径下的兼容存档，请先关闭游戏、保留原档，再把副本复制到同一资源对的存档目录，不覆盖现有文件。用副本验证冷启动能否读回。密钥留在本机。

核心不应用本地化启动器的机器码补丁。CJK 回退字体可以补缺字，但不能还原缺失的游戏字体。资源和运行兼容性须按具体发行版确认。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
