---
title: 用户指南
description: 全面了解 ShinkiEMU Manager 的游戏库管理、核心更新、商店下载、Luau 补丁与存档互通。
---

# 用户指南

本指南详细介绍 ShinkiEMU Manager 的日常功能。如果是初次安装，建议先阅读 [3 分钟快速上手](quick-start.md)。

## 界面语言与主题

Manager 内置简体中文、英文和日文界面，在**设置 → 外观 → 语言**中即时切换。“跟随系统”会根据操作系统区域自动选择（中文环境显示简体中文，日文环境显示日文，其余环境显示英文）。Manager 自带中文字体与日文字体，即使系统未安装 CJK 字体也能清晰显示，并按所选语言切换对应字形。

游戏标题、简介与补丁说明来自社区元数据，优先按当前界面语言显示（缺日文时回退英文，再回退简体中文）。主题支持浅色、深色及自定义强调色。

## Manager 更新检查

在**设置 → 网络**中允许联网后，Manager 每次启动会在后台静默检查新版本，不阻塞启动，网络失败时不弹窗打扰。你也可以在**设置 → 更新**点击“立即检查”。

- 更新源为 `Project-Shinkirou/ShinkiEMU` 的 GitHub Releases（按 `manager-v<semver>` 比较）；使用预发布版本时默认开启“接收预发布版本”。
- 发现新版本后，Manager 会展示版本号、发布说明与当前平台的浏览器下载入口，不自动下载或静默覆盖安装。
- 选择“忽略此版本”会记录在本机，之后不再提醒该版本；选择“稍后”仅在本次会话隐藏。

## 游戏库与核心管理

### 扫描本地游戏

在**设置**中添加存放合法游戏的根目录并扫描游戏库。点击作品可查看自动识别的引擎核心与核心专属配置；若某部作品使用加密归档，仍需在核心配置中提供本机私有密钥（密钥始终只留在你的设备上）。核心升级后，已废弃的旧配置项会自动清理。

### 安装与更新桌面核心

桌面端核心与 Manager 独立发布和更新：

1. 在**设置**中允许联网；
2. 打开**核心**页并刷新目录（目录记录每个核心的版本、ABI、下载地址与 SHA-256 哈希）；
3. 安装或更新当前平台所需的核心。

若更新涉及当前会话已加载的核心库，请先关闭正在运行的游戏，再按提示重启 Manager。请保留完整核心目录中的 `core.json`、运行依赖、可选 `Resources/` 与许可证文件。

## 商店下载（Steam / DLsite / DMM）

允许联网后，打开**添加游戏 → 商店下载**，可直接在 Manager 内下载你账号已拥有的 PC 版作品。登录凭据（Cookie 与刷新 Token）仅保存在操作系统的安全密钥库（Windows 凭据管理器、Linux Secret Service、macOS/iOS 钥匙串、AndroidKeyStore）中，不写入日志或普通安装记录；退出登录即立即清除。

### Steam

- **登录方式**：支持账号密码 + Steam Guard 邮箱/手机令牌确认、无需密码的手机客户端扫码登录，桌面端还可直接导入本地 Steam 客户端的已缓存登录。
- **下载与维护**：搜索已购作品后可选择内容仓库（默认选中 Windows 仓库，便于手机端跨平台游玩）与分支。下载完成后会自动入库并识别核心。支持**检查更新**（基于仓库清单做增量复用）与**校验并修复**，取消操作会保留断点校验标记。请保留游戏目录内的 `.DepotDownloader` 目录以便后续增量更新。

### DLsite 与 DMM GAMES / FANZA

- **网页登录**：点击“打开登录页”会使用系统原生 Web 组件（Windows WebView2、macOS/iOS WKWebView、Linux WebKitGTK、Android WebView）打开官方登录页，直接完成正常登录、CAPTCHA 或两步验证。登录完成后 Manager 自动读取会话 Cookie 并关闭空白私有网页容器，无需手动复制 Cookie。
- **DLsite 下载**：展示账号下 DLsite Play 的已购 PC 作品，仅下载原版 PC 文件（不拉取 Play 网页阅读器媒体）。ZIP 压缩包经路径安全校验后自动解压；RAR 与自解压安装包会保留原样并提示手动解压或运行官方安装程序。
- **DMM GAMES / FANZA**：当前仅开放登录，下载功能尚未提供。

新下载的商店游戏默认位于 `<游戏库目录>/.shinki/store/<商店>/installs/`。Manager 不移除或绕过商店的 DRM、启动令牌或授权检查。

## 兼容性列表与元数据同步

默认元数据源为 `https://project-shinkirou.github.io/data/shinkiemu/v1/`，可在**设置**中手动同步（损坏或校验失败的快照不会覆盖本地有效缓存）。

查看兼容性报告时，请核对 **VNDB 发行版 rID、核心、平台、架构与 desktop/device/simulator 变体**，不同平台与架构的结论相互独立。欢迎通过[兼容性报告表单](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)分享你的实测结果。

## Luau 补丁

ShinkiEMU 的补丁页支持两种来源，共用同一沙箱运行时与事务保护：

- **在线补丁**：从官方审核的补丁库选择，绑定具体 VNDB 发行版与源文件 SHA-256 指纹；
- **本地补丁**：点击**加载本地补丁…**打开 `.luau` 脚本或 ZIP 包（未绑定官方目录的脚本会标记为未经审核）。

补丁脚本仅在你主动点击应用后执行；允许的下载必须经由 HTTPS 且严格核对大小与 SHA-256，缓存于 `<game>/.shinki/patch/`。资源补丁不允许新增或修改 EXE。

### 输出模式与备份还原

- **新目录模式（默认）**：将转换结果输出到选定父目录下的 `localized-output`（若重名则自动顺延为 `localized-output-1` 等），原游戏目录完全保持不变，也不复制原存档。
- **原位覆盖模式**：先在私有暂存区完成转换，再将被修改或删除的原文件备份到 `<game>/.shinki/backup/<id>/`（附带 `manifest.json` 哈希清单）。随时可点击**还原最近一次补丁**，且不会覆盖你之后手动改过的文件。

> [!TIP]
> 部分补丁需要更新核心才能生效（例如新 Musica 补丁需要 Musica 核心 `beta.5` 及以上）。Manager 会在应用和启动前自动核对核心能力，如提示升级核心，请先在**核心**页更新后重试。

## 数据目录与原版存档互通

ShinkiEMU 自身在游戏目录内生成的所有文件（缓存、补丁备份、产品存档等）统一存放在 `<game>/.shinki/` 下，不会污染商业游戏根目录。应用自身数据目录可在**设置**中一键打开，桌面端也支持通过环境变量 `SHINKI_EMU_DATA_DIR` 指定绝对路径。

各核心的存档路径因引擎而异：

- **外部核心**（RFVP、Siglus、Artemis、KiriKiri2、Ren’Py、impacto 等）：沿用对应上游项目的存档位置与格式，详见各核心文档。
- **Shinki Musica**（`shinki.emu.musica`）：在 `<game>/.shinki/musica/minori/<title>/` 下直接读写原版兼容的 `.sav` 与 `.png` 文件，可与 Windows 原版 `Documents\minori\<title>\` 双向复制互通（详见 [Musica 核心文档](cores/shinki-emu-musica.md)）。

备份或跨设备交换存档前，请务必先关闭正在运行的游戏。
