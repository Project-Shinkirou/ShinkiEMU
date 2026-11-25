---
title: 快速上手
description: 3 分钟下载安装 ShinkiEMU Manager、添加游戏、安装原生核心并开始游玩。
---

# 快速上手

这份指南帮助你在 3 分钟内用 ShinkiEMU 启动第一部视觉小说。

> [!IMPORTANT]
> **免费开源声明**：ShinkiEMU 是完全免费的开源软件，请仅从[官方门户](https://project-shinkirou.github.io/zh-cn/shinkiemu/#download)或 [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) 下载。如果你曾为此软件付费，请申请退款并举报倒卖卖家。

## 第一步：按平台下载 Manager

从[下载页](https://project-shinkirou.github.io/zh-cn/shinkiemu/#download)或 [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) 获取对应平台的最新版本（当前为 **Manager 0.1.0-beta.8**）：

| 平台 | 安装包说明 | 核心方式 |
|---|---|---|
| **Windows x64** | 下载 ZIP 解压至可写目录，运行 `shinki-emu-manager.exe` | 在应用内核心页按需安装 |
| **Linux x64** | 先安装[运行库](troubleshooting.md#linux-桌面运行库)，解压后运行 `./shinki-emu-manager` | 在应用内核心页按需安装 |
| **macOS（Apple Silicon / Intel）** | 解压 ZIP 后打开 `ShinkiEMU.app` | 在应用内核心页按需安装 |
| **Android** | 安装通用正式签名 APK（含 `arm64-v8a` / `x86_64`） | 已内置 8 个核心 |
| **iOS** | 真机 / 模拟器 ad-hoc 测试包（需自行签名安装） | 已内置 6 个核心 |
| **OpenHarmony** | 实验性测试签名 HAP（真机启动与零售安装尚未验证） | 已内置 Musica 核心 |

解压桌面包时，请保留压缩包内的全部依赖、资源与许可文件，不要单独复制可执行文件。

## 第二步：首次启动确认

首次打开 Manager 时，界面会展示中英双语**免费开源与防倒卖提示**。完整阅读并等待提示计时完成后，勾选“我已阅读 / I have read this”即可进入主界面。之后可随时在**设置 → 关于**重新查看该提示。

- 界面默认跟随系统语言（支持简体中文、英文、日文），可在**设置 → 外观 → 语言**即时切换。
- 如需同步核心目录、元数据或使用商店下载，请在**设置 → 网络**中允许联网。

## 第三步：添加游戏与安装核心

你可以通过以下任意一种方式添加合法取得的游戏：

1. **导入本地游戏**：在**设置 → 游戏库目录**中添加存放游戏的文件夹，然后点击扫描游戏库。
2. **从商店直接下载**：允许联网后，打开**添加游戏 → 商店下载**，登录你的 Steam 或 DLsite 账号并下载已拥有的 PC 作品。

在桌面端，前往**核心**页刷新目录并安装游戏所需的核心（Android / iOS / OpenHarmony 直接使用内置核心）。

## 第四步：启动游玩

回到**游戏库**，点击游戏卡片确认匹配到的核心与可选配置，点击**启动**即可开始游玩。

- 想确认某部作品在当前平台的实测状态，请先查阅[兼容性列表](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。
- 需要了解商店登录细节、Luau 补丁、原版存档互通或更新设置，请继续阅读[用户指南](user-guide.md)。
- 遇到启动、运行库或下载问题，请查看[故障排查](troubleshooting.md)或在 [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions) 提问。
