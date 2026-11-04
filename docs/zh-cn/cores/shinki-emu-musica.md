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

核心 owner 运行 SC VM、硬件离屏合成和原生音频。默认 `launch_mode=direct` 执行配置的原版入口脚本；`title` 需要发行版前端数据。输入和帧轮询不推进 VM。逻辑画布尺寸与宿主缩放分开配置。设置页和存档卡片仍保留已恢复的原生布局，标题点击区和鉴赏列表不再要求 1280 宽画布。影片 ASS 使用真实 movie 时钟和明确字体，此路径不接受 BGM 字幕映射。

未知命令、未支持演出和设备故障明确报错。音频当前采用有界完整资源解码，不是任意长媒体流式解码。

## 前端数据与归档发现

按“原版文件解析 → 发行版 Metadata 配置 → Luau 修正非标准资源”的顺序取数。核心没有作品分派、内置 CLEAR 标记、回想脚本表、影片文件名表或角色名单。

| 数据 | 原版来源与当前覆盖范围 | 无法取得时的配置 |
| --- | --- | --- |
| 标题图、菜单动作、矩形、变体与解锁 | 脚本菜单直接运行原版 SC。已恢复的 PAZ 目录只提供图像名，不能推断动作、矩形与全局标记；尚不解析原生 EXE 菜单表，也不根据图像命名猜菜单。 | `frontend.titles`：实际背景/悬停 URI、按钮与全局变量条件 |
| 回想 | `frontend.replay_index` 指定原版 SC 索引，按源顺序读取 `.chain file.sc`。不把普通剧情文件名当成回想；此索引读取器不支持 `#label` 目标。 | `frontend.replays`：有序标签、脚本 URI 与条件 |
| 影片 | 扫描原版脚本目录，发现仅含 movie/wait/end 的独立 `.movie` 包装脚本，且目标影片必须存在；也可用 `movie_index` 指定原版 `.chain file.sc` 索引。含影片的普通剧情脚本不作为入口。 | `frontend.movies`：需要额外初始化的发行版可显式提供脚本入口 |
| 音乐/CG | 音乐列表来自实际 `bgm` 归档的 `.ogg`。普通图像目录无法区分 CG 与背景。 | 找不到原版音乐时用 `frontend.bgm`；CG 索引用 `frontend.cg` |
| 角色 | 实际 `voice` 归档 `.ogg` 文件名取 `-`/`_` 前一段；无分隔符时去扩展名与末尾数字。核心不解释角色名。 | `voice_characters` JSON 数组 `{prefix,label,enabled}` 补充名称、顺序与初始开关 |
| 归档角色 | 私有 `key.toml` 的实际 `archive_keys`；probe/open 共用 PAZ 0/1/2 结构、解密、索引/条目边界校验。 | `archive_keys.<role>.required` 默认 **true**；可选归档显式写 false。缺可选文件跳过，已存在但损坏的文件仍报错 |

自动 probe 读取游戏已有的默认 `key.toml`，挂载后验证脚本归档中有可解析的 SC 命令。加密 PAZ 没有通用魔数，不能只靠文件名识别；没有默认密钥便不自动匹配。显式选择外部 `profile_file` 后仍走同一挂载校验，probe 不安装密钥。CLI 导入默认每个已声明角色 `required=true`，保留 keys.v1/profile.v1 输入的显式 `required=false`；GARbro 没有可选标志，需要修改导入后的私有 key.toml。缺必需归档时，错误列出角色与期望路径。

descriptor 声明了中文标签与默认值：`frontend` 是 JSON 字符串（默认空），`voice_characters` 是 JSON 字符串（默认 `[]`），另有既有画布与启动字段。发行版值写在 `Metadata/engines/presets/<rid>/shinki.emu.musica.json`，使用 `shinki.emu.presets.v1`；`configuration.frontend` 为下列对象编码后的字符串：

```json
{
  "titles": [{
    "background": "musica:/sys/welcome.png",
    "hover": "musica:/sys/highlight.png",
    "requires": {},
    "buttons": [
      {"rect": [10, 20, 120, 40], "action": "new_game"},
      {"rect": [10, 80, 120, 40], "action": "memories"},
      {"rect": [10, 140, 120, 40], "action": "exit"}
    ]
  }],
  "replay_index": "musica:/scr/memory-index.sc",
  "replays": [],
  "movies": [{"label": "Credits", "resource": "musica:/scr/credits.sc", "requires": {"finished": 1}}],
  "cg": [{"label": "Memory", "resource": "musica:/bg/memory.png"}]
}
```

标题按顺序匹配，采用最后一个 `requires` 全部相等的变体；不存在的全局变量按零处理。按钮矩形为逻辑 `[x,y,width,height]`，动作支持 `new_game`、`load`、`config`、`memories`、`exit`。鉴赏条目包含 `label`、`resource` 和可选 `requires`；空类别不显示。方向键选择，Enter/Space 执行，Escape 返回，Backspace 停止鉴赏音乐。文字与鼠标行共用画布比例布局，每页 12 项。不再内置曲目编号和 CG 页数。

每个 JSON 字段遵守 ABI 的 4096 字节配置上限，大目录应引用原版 SC 索引。自动影片扫描最多读取 4096 个脚本、合计 64 MiB 展开源文件；更大目录需指定 `movie_index`。发现成功时，原版列表决定成员与顺序，preset 中同 URI 的标签与解锁条件仍保留。此读取器不把 SC 条件控制流反推成解锁公式，相关规则需由 preset 描述。

**配置迁移：**从 Manager 已保存配置删除 `voice_ren`、`voice_sui`、`voice_aya`、`voice_tou`、`voice_mot`，改用 `voice_characters` 中实际前缀；没有旧字段别名。已显式保存的 `launch_mode=title` 需要补 `frontend`，原版脚本菜单则切为 `direct`。eden/Perseus 需用发行版 preset 提供实际菜单图、通关条件、回想/影片入口和角色顺序，不在核心保留特例。合成 fixture 覆盖两种标题图大小写布局，以及缺可选归档的另一套 640×360 数据；这不代表商业发行版已验收。原生格式仍有五个语音设置键，按声明角色顺序映射，多出的角色写前端设置 `voice_<prefix>`；回归测试对同一角色集合与顺序逐字节比较迁移前后的原生设置编码。

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

`directory` 可以是游戏内相对路径，也可以显式指定原版存档的绝对路径（含 Documents）；核心不自动搜索 Documents。`system_format` 可选 `Rc4` 或 `Blowfish`；`extended` 选择已恢复的扩展检查点布局和快速/自动槽位组，与作品名无关。`append_slot_prefix` 补充源文件中尚未出现的追加篇命名空间。`voice_keys` 按配置顺序将前五个角色映射到原生设置键，其余角色使用前端设置键。不支持的描述版本会明确报错。

能从原版文件确定的参数优先采用解析结果；注册表路径、缺失的追加篇前缀、设置键和命令计数周期由描述补充。解析完成的描述保存在 `.shinki/musica/save-identity.json`，尚未建立槽位时重启也不会改变配置。前端字段迁移见上节。原有 minori 目录保持原位；旧文件不足以确定完整身份时，启动报 `SHINKI_EMU_MUSICA_SAVE_CONFIG`，要求补充 `native_save`，不会静默切换并隐藏旧进度。

Windows 上，`serial_key` 指定 HKLM 注册表键，对其 ANSI `SerialCode` 字节（到第一个 NUL 为止）取 CRC32。读不到时，依次使用显式 `prefix`、已有有效压缩文件的前缀，最后使用非严格校验的零前缀。四字节前缀不能反推出注册表键或保存目录。例如 eden 风格描述使用原版 `eden` / `edenPM` 命名、`eden_en 1.00` 签名、RC4 与基础槽；Perseus 风格描述提供 `perseus_`、`perseus 1.00`、Blowfish、扩展槽及其注册表键。这些属于发行版配置，核心没有对应的作品表。

格式已恢复时，全局变量、设置、CG、已读记录与槽位保持原生字节格式。显式指定原版目录时直接写回该目录；旧的 `.shinki/musica/minori/<folder>/` 保持原位，可与原版安装互相复制兼容文件。`system.dat`、`sysparams.txt` 和前缀保持原版表示；zlib 压缩字节可能不同，解压内容相同。未映射的设置字节原样保留，纯前端设置仍写入 `.shinki/musica/musica-settings.txt`。

**无法解析原生身份且没有描述时**，仍创建完整后端，将全部状态真实写入 `<game>/.shinki/musica/generic-v1/`，按游戏目录隔离。`slot_NNNN.sav` 使用独立的 `shinki.musica.save.v1` 签名和已恢复的基础检查点正文；辅助文件采用 RC4 全局表、Tab 分隔设置及零前缀 zlib CG/已读表。这是 ShinkiEMU 通用格式，不代表未知原版 exe 可以读取。仅有熟悉的 exe 文件名不会改变选择。切换描述会选择另一个后端，不删除或合并旧文件，请保留备份。

早期 `.shinki/musica/saves/` 中的 JSON 设置、快速槽游标和全局进度，可一次性、不覆盖地导入**两种后端**。只有写入成功后才把源文件改名为 `.imported`；已有目标文件优先。旧 beta `.asav`（`AMUSSV06` 及更早）仍不兼容，两个旧位置中的文件都保持不动，请备份并新建槽位。

旧进度哈希依次匹配原版入口 SC 的 `.setglobal` 变量名、`frontend` 的条件键、preset 的 `legacy_progress_flags` JSON 字符串数组（默认 `[]`），没有内置变量名表。匹配不全也能打开：已匹配项照常迁移，不覆盖现有全局变量；原始 JSON 保持不变，`.shinki/musica/saves/global-progress-unmapped.json` 列出缺失哈希及游戏目录。补齐声明后再次启动即可继续迁移，期间产生的全局变量修改保持不变；全部匹配后才将源文件改名为 `.imported`。无待迁移诊断时，原有原生目标文件仍优先。运行时只按实际 `.setglobal` 赋值为一的语义记录进度，不匹配变量拼写；菜单条件仍按配置的整数值判断。

旧 `configuration.json` 若含 `character_voice_enabled`，需按发行版声明的角色顺序，显式转换为 `character_voices` 对象列表。转换前保留原始备份；严格导入器不猜五角色名单。

检查点恢复脚本变量、背景、组合前景、立绘位置与偏移、BGM、消息历史，以及小说模式（`CP1=2`）的整页文本和 `L4_n` 位置。读档重新开始当前消息及语音；播放时钟、翻译文本和菜单焦点不保存。不支持的效果或附加图层明确拒绝，不回退到内部快照。

基础布局使用自动槽 0、快速槽 1–9，`AppendPlaying` 选择配置的追加篇前缀；扩展布局使用快速槽 0–9、自动槽 10–19、手动槽 20 起。传入对应源文件或描述后，eden/Perseus 的轮转行为保持不变。自动存档按配置的命令计数周期、四项以内的选择前及视频前触发；`exitFromEvent`、`prohibitAutoSave`、`prohibitSaveLoad` 的禁止规则保持不变。先写缩略图，再写槽位，两者分别原子替换。

[ReMinori](reminori.md) 是另一个实现，执行器、配置和存档各自独立。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
