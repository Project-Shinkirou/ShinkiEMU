---
title: Manager 0.1.0-beta.4
description: 界面语言切换、Android 内置核心和原生发行包。
---

# Manager 0.1.0-beta.4

Manager 新增简体中文、英语和日语界面，所选语言会保存并在下次启动时恢复。

Android Manager 现可同时内置 Artemis、Siglus 和 RFVP。Artemis 改用 Luau，内置核心与 Manager 共用图形和 VM 依赖。Android 下载文件改为可直接安装的 APK。各平台发行包均包含 ShinkiEMU 应用图标，文件名不再含有 `adhoc`。

核心发行目录 PR 由受信任的 metadata 工作流校验，通过后自动合并。本次只发布 Manager；已有的 beta.3 可下载核心仍使用兼容的原生 C ABI v3。Android 核心的新版本随 Manager APK 一并提供，不单独发布核心。

HarmonyOS 主机未在集成截止前通过验收，因此不纳入本次发行。
