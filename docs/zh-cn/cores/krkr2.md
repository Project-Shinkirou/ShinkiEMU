---
title: KrKr2
description: 移动端 Manager 的独立原生 KrKr2 核心。
---

# KrKr2

`plugin_id: krkr2` · 引擎家族：KiriKiri

上游：[2468785842/krkr2](https://github.com/2468785842/krkr2)。适配 fork：[Project-Shinkirou/krkr2，shinki 分支](https://github.com/Project-Shinkirou/krkr2/tree/shinki)。

## 格式与选择

探测独立 `startup.tjs` 时置信度为 9400，`data.xp3` 为 9300。现有 `kirikiroid2-web` 使用独立 ID，匹配时仍以 9500 优先选中。要使用此实现，请手动选择 `krkr2`。探测结果不代表具体游戏兼容性。

## 原生行为

适配器运行此上游自己的模块化 C++ TJS VM、KAG、渲染、插件、字体和存档/偏好设置路径，不转发给 kirikiroid2-web。Android/iOS 静态链接独立的原生符号闭包，Android 的 Java/JNI 包也单独命名。原始游戏纹理通过不可变 CPU 帧租约输出；暂停、恢复和输入交给原生 owner 线程，取消和关闭会等待其工作线程退出。

本分支接入 Android/iOS Manager 路径，不声明独立桌面或移动端核心包。构建需要目标 SDK、Bison 和本核心的 vcpkg 依赖闭包。发布验收和真实游戏兼容性另行记录。原许可与修改范围见 fork 的 `LICENSE`、`MODIFICATIONS.md`。
