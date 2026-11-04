---
title: Musica
description: Musica 的格式、原生行为与限制。
---

# Musica

`plugin_id: shinki.emu.musica` · 引擎家族: Musica

这是 ShinkiEMU 仓库内的自有核心，没有独立外部 fork。实现见 [Source/Families/shinki-emu-musica](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/Families/shinki-emu-musica)，来源提交见 [ORIGINS](../../../ORIGINS.md)。

## 格式

仓库内的 Musica 核心读取 PAZ 和原生 SC，处理分卷、角色密钥、变换及有界解压。私有 `key.toml` 使用 `shinki.emu.musica.keys.v2`，算法参数和密钥都要明确提供。探测识别不会安装密钥。标准 loose SC 和配置的 PNG 替换文件限制在游戏根目录内。

## 原生行为与限制

核心 owner 运行 SC VM、硬件离屏合成和原生音频，默认先显示标题菜单，再进入剧情。输入和帧轮询不推进 VM。现有呈现包含 1280×720 布局及 Noto Sans JP 字体绑定，宿主缩放不改游戏坐标。影片 ASS 使用真实 movie 时钟和明确字体，此路径不接受 BGM 字幕映射。

未知命令、未支持演出和设备故障明确报错。音频当前采用有界完整资源解码，不是任意长媒体流式解码。

## 存档

Musica 从原版文件解析存档身份，不按 exe 文件名或作品名选择。解析范围包括游戏根目录、`save/`、`savedata/` 和已有的 `.shinki/musica/minori/*/`。可解码的 `system.dat` 确定 RC4/Blowfish 封装；有效的编号 `.sav` 提供实际目录、文件名前缀与数字宽度、NUL 结尾的容器签名、四字节前缀，以及基础/扩展检查点布局。只有两套命名都存在时，才能从文件中推导追加篇命名空间。单凭签名或 SerialCode 无法确定其余参数。

| 文件 | 内容 |
| --- | --- |
| `system.dat` | 全局标志和通关状态 |
| `sysparams.txt` | 设置、快速存档和自动存档游标（Tab 分隔，CRLF） |
| `<script>.tbr` | 单个脚本的已读消息位图 |
| `cgv.dat` | 已看过的 CG 列表 |
| `<prefix><number>.sav` | 按解析或配置的命名模板保存消息检查点与历史 |
| 同名 `.png` | 不透明 RGB 区域均值缩略图：基础布局 128×80，扩展布局 106×60 |

原版文件不足以确定格式时，由发行版本绑定的 Metadata preset（`Metadata/engines/presets/<rid>/<plugin_id>.json`，`shinki.emu.presets.v1`）提供 `configuration.native_save`，值为 JSON **字符串**。解码后的描述如下：

```json
{
  "version": 1,
  "directory": "save",
  "slot_prefix": "slot_",
  "append_slot_prefix": null,
  "slot_digits": 4,
  "signature": "engine-release 1.00",
  "system_format": "Rc4",
  "extended": false,
  "serial_key": null,
  "prefix": 0,
  "auto_save_cycle": 200,
  "voice_keys": null
}
```

`directory` 可以是游戏内相对路径，也可以显式指定原版存档的绝对路径（含 Documents）；核心不自动搜索 Documents。`system_format` 可选 `Rc4` 或 `Blowfish`；`extended` 选择已恢复的扩展检查点布局和快速/自动槽位组，与作品名无关。`append_slot_prefix` 补充源文件中尚未出现的追加篇命名空间。`voice_keys` 可将现有五组前端语音开关映射到原生设置键，本切片不改变前端资源绑定。不支持的描述版本会明确报错。

能从原版文件确定的参数优先采用解析结果；注册表路径、缺失的追加篇前缀、设置键和命令计数周期由描述补充。解析完成的描述保存在 `.shinki/musica/save-identity.json`，尚未建立槽位时重启也不会改变配置。本次只增加配置字段，不删除旧字段。原有 minori 目录保持原位；旧文件不足以确定完整身份时，启动报 `SHINKI_EMU_MUSICA_SAVE_CONFIG`，要求补充 `native_save`，不会静默切换并隐藏旧进度。

Windows 上，`serial_key` 指定 HKLM 注册表键，对其 ANSI `SerialCode` 字节（到第一个 NUL 为止）取 CRC32。读不到时，依次使用显式 `prefix`、已有有效压缩文件的前缀，最后使用非严格校验的零前缀。四字节前缀不能反推出注册表键或保存目录。例如 eden 风格描述使用原版 `eden` / `edenPM` 命名、`eden_en 1.00` 签名、RC4 与基础槽；Perseus 风格描述提供 `perseus_`、`perseus 1.00`、Blowfish、扩展槽及其注册表键。这些属于发行版配置，核心没有对应的作品表。

格式已恢复时，全局变量、设置、CG、已读记录与槽位保持原生字节格式。显式指定原版目录时直接写回该目录；旧的 `.shinki/musica/minori/<folder>/` 保持原位，可与原版安装互相复制兼容文件。`system.dat`、`sysparams.txt` 和前缀保持原版表示；zlib 压缩字节可能不同，解压内容相同。未映射的设置字节原样保留，纯前端设置仍写入 `.shinki/musica/musica-settings.txt`。

**无法解析原生身份且没有描述时**，仍创建完整后端，将全部状态真实写入 `<game>/.shinki/musica/generic-v1/`，按游戏目录隔离。`slot_NNNN.sav` 使用独立的 `shinki.musica.save.v1` 签名和已恢复的基础检查点正文；辅助文件采用 RC4 全局表、Tab 分隔设置及零前缀 zlib CG/已读表。这是 ShinkiEMU 通用格式，不代表未知原版 exe 可以读取。仅有熟悉的 exe 文件名不会改变选择。切换描述会选择另一个后端，不删除或合并旧文件，请保留备份。

早期 `.shinki/musica/saves/` 中的 JSON 设置、快速槽游标和全局进度，可一次性、不覆盖地导入**两种后端**。只有写入成功后才把源文件改名为 `.imported`；已有目标文件优先。旧 beta `.asav`（`AMUSSV06` 及更早）仍不兼容，两个旧位置中的文件都保持不动，请备份并新建槽位。

检查点恢复脚本变量、背景、组合前景、立绘位置与偏移、BGM、消息历史，以及小说模式（`CP1=2`）的整页文本和 `L4_n` 位置。读档重新开始当前消息及语音；播放时钟、翻译文本和菜单焦点不保存。不支持的效果或附加图层明确拒绝，不回退到内部快照。

基础布局使用自动槽 0、快速槽 1–9，`AppendPlaying` 选择配置的追加篇前缀；扩展布局使用快速槽 0–9、自动槽 10–19、手动槽 20 起。传入对应源文件或描述后，eden/Perseus 的轮转行为保持不变。自动存档按配置的命令计数周期、四项以内的选择前及视频前触发；`exitFromEvent`、`prohibitAutoSave`、`prohibitSaveLoad` 的禁止规则保持不变。先写缩略图，再写槽位，两者分别原子替换。

[ReMinori](reminori.md) 是另一个实现，执行器、配置和存档各自独立。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
