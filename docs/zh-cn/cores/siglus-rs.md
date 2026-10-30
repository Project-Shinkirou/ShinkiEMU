---
title: Siglus
description: Siglus 的格式、原生行为与限制。
---

# Siglus

`plugin_id: siglus_rs` · 引擎家族: SiglusEngine

上游：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)。适配 fork：[Project-Shinkirou/siglus_rs 的 shinki-android-isolation 分支](https://github.com/Project-Shinkirou/siglus_rs/tree/shinki-android-isolation)。

## 格式

`Scene.pck` 汇集 scene chunk 及包级 include 属性/命令。包偏移相对包起点，解码后 `.ss` 中的偏移相对 chunk 起点。字符串索引的偏移和长度按 UTF-16 code unit 计数，不是字节；存储字串带与索引有关的逐 word 变换。scene、label 和 command entry 表各自参与 VM 控制流。

## 原生行为与限制

fork 保留 Siglus VM、renderer、文字/键盘输入和原生存档。适配层拥有执行循环并发布 CPU 帧。原生配置支持 original/chs 成对资源和 G01/movie 选择。

Android Manager 静态嵌入 Siglus、RFVP、Artemis 和 Musica。Siglus 与 RFVP 使用和 Manager 相同的 wgpu 30 运行时。两者的原生 WMV 解码器保留 Rust API，不在 Manager 中启用可选 `c-abi` 导出，避免重复的 `wmv2_decoder_*` 符号。

## 存档与密钥

存档位置与原版引擎一致：`<游戏>/savedata/` 存放 `NNNN.sav` 存档位及缩略图、`global.sav`、`config.sav`、`read.sav` 和 Twitter 状态；截图和导出文件写在游戏目录下的相对路径。恢复出的密钥按上游写入 `<游戏>/key.toml`。`chs` 资源对与原版资源对共用 `savedata/`，和打过汉化补丁的原版行为相同。游戏目录需要可写。

0.1.0-beta.1 和 beta.2 把存档放在 `%LOCALAPPDATA%\siglus_rs\saves\<哈希>\siglus-<语言>-<标识>\`（macOS 为 `~/Library/Application Support/siglus_rs/saves/…`，Linux 为 `$XDG_DATA_HOME` 或 `~/.local/share/siglus_rs/saves/…`）。首次启动该游戏时，核心会把这个目录一次性导入游戏目录：已有文件不会被覆盖，旧目录随后改名为 `….imported`。如果当时设置过已移除的“存档目录”选项，核心已无从找到那个目录，请手动把其中每个 `siglus-*` 文件夹的内容（`savedata/` 及截图文件）复制到游戏目录，注意不要覆盖想保留的文件。

本地化发行版的 wrapper 不属于嵌入运行时。核心不应用 wrapper 机器码补丁，也不补造缺失字体；包解码、资源配置和运行行为都要按具体发行版确认。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
