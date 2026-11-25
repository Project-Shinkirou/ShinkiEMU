---
title: 故障排查与常见问题
description: Linux 运行库安装、Android GPU 限制、DLsite 下载诊断、存档迁移与补丁升级排查指南。
---

# 故障排查与常见问题

遇到启动失败、黑屏、下载异常或存档问题时，可以先按以下分类快速排查；也可以同时查看官网[常见问题（FAQ）](https://project-shinkirou.github.io/zh-cn/shinkiemu/faq/)。

## Linux 桌面运行库

Linux 发行包复用系统提供的图形、音频与 WebView 运行库。在 Ubuntu 24.04 / Debian 系桌面运行前，建议安装以下运行包：

```sh
# Manager 图形界面与商店网页登录（WebKitGTK 4.1 + GTK 3）
sudo apt-get install -y libwebkit2gtk-4.1-0 libgtk-3-0t64 libfontconfig1 libasound2t64

# 常用核心运行库（OpenGL / X11 / Pango / OpenMP / DRM）
sudo apt-get install -y libgl1 libglx0 libopengl0 libx11-6 libgomp1 libdrm2 libpangocairo-1.0-0 libpangoft2-1.0-0

# 使用 RFVP 核心播放原生影片时需要 GStreamer 1.x 及解码插件
sudo apt-get install -y libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav
```

若使用其他发行版，请安装对应等价包，并确保已配置带 GPU 驱动的 X11 或 Wayland 桌面会话。每个核心包内的 `build.json` (`system_dependencies`) 均列出了该核心所需的确切系统 SONAME。

## 首次启动的免费开源提示无法勾选

首次启动引导中的中英双语提示需要**完整可见并累计阅读 8 秒**后才能勾选继续。若进度条暂停，通常是因为窗口过矮导致提示图片被裁切或滚动区域未完整露出：

- 拉高窗口高度，或向下滚动使整张提示文字完全处于可视区域内，计时即会继续；
- 切换浅色/深色主题会重新渲染提示图，但已累计的阅读时间会保留。

## Android 模拟器界面卡在首帧

在 Android 模拟器上若使用软件渲染（SwiftShader），Slint 1.18.1 会出现仅渲染首帧、后续点击无刷新的现象。

- **解决方法**：启动模拟器时启用硬件加速（`-gpu host`），或直接使用实体 Android 设备测试。

## 商店下载常见问题

### Windows / Linux 无法打开 DLsite 或 DMM 登录页

- **Windows**：需要 Microsoft Edge WebView2 运行时（Windows 11 默认自带，Windows 10 请从微软官网安装 Evergreen WebView2 Runtime）。
- **Linux**：确认已安装 `libwebkit2gtk-4.1-0`（包含 GTK 3）。

### 下载完成后提示需要手动解压或运行安装程序

如果界面提示 **Downloaded archives need manual extraction or installer run**，说明商店分发的是 RAR 压缩包或官方自解压安装程序。ShinkiEMU 不会自动执行下载的可执行文件：请点击“打开下载目录”自行解压或安装，再将最终可运行的游戏目录添加到游戏库。

### 更新状态显示“Update status unavailable”

当商店接口未提供可靠的本地构建标识（例如当前 DLsite 接口）或网络暂时不可用时，Manager 会正常显示更新状态不可用。Steam 作品可点击刷新游戏库重新查询。

### 协助排查 DLsite 作品无法解析下载计划

若某部已购 DLsite PC 作品无法解析下载计划，可在已登录的浏览器中复现并导出含响应正文的 HAR：记录从 `https://play.dlsite.com/api/v3/download?workno=<作品编号>` 起至重定向及 `/home/download`、`/home/serial` 或分卷列表请求结束的链路。**分享前务必彻底删除 Cookie、序列号与签名 URL Token，切勿直接公开上传原始 HAR 文件。**

## 补丁提示需要更新核心

应用补丁或启动已打补丁的游戏时，若提示当前核心缺少所需能力（例如新 Musica 原生补丁要求 Musica 核心 `beta.5` 及以上，或 RFVP 原生脚本补丁要求脚本存档隔离与配置字体优先）：

1. 打开**核心**页刷新目录并更新对应核心；
2. 如有运行中的游戏会话，先关闭游戏并按提示重启 Manager；
3. 重新启动游戏或应用补丁。

## 存档与旧版数据迁移

- **原版存档互通**：以 `shinki.emu.musica` 为例，可将 Windows 原版 `Documents\minori\<title>\` 下的文件复制到 `<game>/.shinki/musica/minori/<title>/`（反向亦然）。
- **早期 beta 的 `.asav` 文件**：Musica 早期测试版的 `.asav`（`AMUSSV06` 及更早）与当前原版兼容存档不兼容。ShinkiEMU 不会修改或删除 `.shinki-musica/saves/` 或 `.shinki/musica/saves/` 中的旧文件，请自行备份并建立原生存档。
- **`Manager 0.1.0-beta.7` Windows 版本号显示**：已知历史问题——官方 `beta.7` Windows 二进制内部自报 `0.1.0-beta.6`，自 `0.1.0-beta.8` 起已修复并统一构建期版本校验。

## 仍未解决？

- **使用提问与经验交流**：前往 [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions) 发帖；
- **明确的可复现缺陷**：填写 [Bug 报告表单](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)；
- **外部引擎自身脚本/渲染缺陷**：可同时参考各核心文档中的上游仓库链接，在合适时向上游反馈。
