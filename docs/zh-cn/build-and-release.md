---
title: 从源码构建
description: 开发者本地环境准备、cargo xtask 构建/检查/测试命令与各平台构建入口。
---

# 从源码构建

本指南面向希望从源码编译、调试与测试 ShinkiEMU 的开发者。如果你需要维护者发布流水线、依赖锁定明细、Apple SDK 提取、OHOS 本地交接或源码许可归档规范，请参阅[发布与维护手册](release-runbook.md)。无窗口验收与界面截图见[无窗口运行](headless.md)。

## 工具链与前置依赖

- **Rust 工具链**：工作区使用 Rust 2024 edition，固定版本为 **Rust 1.99.1**（由 `rust-toolchain.toml` 自动选定）。
- **克隆仓库**：需初始化子模块 `ThirdParty/cpal`（外部核心由 `cargo xtask deps` 按 `build/cores.toml` 拉取，不是 Git 子模块）：
  ```sh
  git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU.git
  ```
- **权威清单**：工具与下载版本见 `build/dependencies.toml`，核心与特性见 `build/cores.toml`，目标与 runner 见 `build/targets.toml`。

| 平台 | 主要前置依赖 |
|---|---|
| **Windows x64** | x64 MSVC 开发者环境、CMake、Ninja、Git |
| **Linux x64** | GCC/Clang、CMake、Ninja、pkg-config、NASM、WebKitGTK 4.1 开发包与音频/图形开发库（见下方 apt 命令） |
| **macOS / iOS** | 原生 Xcode（或 Linux 下 cross 0.2.5 + 私有官方 Apple SDK，详见[发布与维护手册](release-runbook.md#apple-manager原生-macos-与-linux-交叉构建)） |
| **Android** | JDK 27、Android SDK platform 37.2、build-tools 37.0.0、NDK r30（`30.0.16248370`）、`cargo-ndk 4.1.2` |
| **OpenHarmony** | DevEco Studio 自带 Native SDK 5.0.5（API 17，指向 `OHOS_SDK_NATIVE`） |

## 常用 `cargo xtask` 入口

从仓库根目录运行，按修改范围选择 `--manager` 或单个 `--core`，避免混入无关核心的原生依赖图：

```sh
# 查看构建矩阵与本地环境诊断（doctor 仅检查，不安装系统包）
cargo xtask matrix
cargo xtask doctor --target x86_64-pc-windows-msvc

# 显式准备锁定的外部核心源码与原生依赖（可用 --core 缩小范围）
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc --core rfvp

# 快速检查、构建与测试
cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
cargo xtask metadata validate
```

- `cargo xtask test` 不带组件参数时运行 `xtask` 与构建辅助库测试；原生核心回归可能需要真实 GPU/音频设备与公开夹具。
- 调试尚未提交的外部核心 fork 改动时，可向 `deps` 传入 `--allow-dirty-sources`（要求工作区 HEAD 已在固定 revision；正式发布拒绝脏工作区）。
- `build.rs` 会在 Cargo `OUT_DIR` 写入 `core-build.json`，各组件构建产物隔离在 `.work/target/<target>/<component>`，构建记录保存在 `.work/build/`。

## 商店插件 Cargo Feature

`shinki-emu-store` 定义对象安全的 provider/session trait、取消机制与密钥库边界。Manager 默认编译 `store-steam`、`store-dlsite`、`store-dmm`。如需仅编译 Steam，可传入 `--no-default-features --features store-steam`。新增商店需实现 provider crate、添加可选依赖与 feature，并在 `manager_app/store.rs` 注册一行 `registry.register(...)`。

## Linux 桌面开发环境

在 Ubuntu 22.04 / 24.04 上安装原生开发包：

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev libxinerama-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev mesa-vulkan-drivers xvfb xauth unzip jq
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav libgtk-3-dev libwebkit2gtk-4.1-dev
```

随后按需构建与打包：

```sh
cargo xtask deps --target x86_64-unknown-linux-gnu
export SHINKI_EMU_PRODUCT_VERSION=0.1.0-beta.8
cargo xtask build --target x86_64-unknown-linux-gnu --manager --release
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-beta.8 --out .work/linux-manager
```

Linux 运行时用户所需的最小运行库列表见[故障排查 — Linux 桌面运行库](troubleshooting.md#linux-桌面运行库)。

## Android 本地测试 APK

配置 `JAVA_HOME`（JDK 27）、`ANDROID_HOME`、`ANDROID_NDK_HOME`（指向 `ndk/30.0.16248370`）后运行：

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo xtask deps --target aarch64-linux-android
cargo xtask deps --target x86_64-linux-android
export SHINKI_EMU_PRODUCT_VERSION=0.1.0-beta.8
cargo xtask build --target aarch64-linux-android --manager --release
cargo xtask build --target x86_64-linux-android --manager --release
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.8 --out .work/android-test
```

打包通用 APK 需要 `arm64-v8a` 与 `x86_64` 来自同一提交的 release 构建记录。未提供正式签名密钥时输出 debug 签名测试包（`ShinkiEMU-<version>-android-universal-debug-signed.apk`）；首次拉完 Gradle 依赖后可设 `SHINKI_EMU_GRADLE_OFFLINE=1` 离线构建。

## 磁盘布局与应用图标约定

- **游戏目录隔离**：ShinkiEMU 在游戏目录内写入的所有内容统一位于 `<game>/.shinki/`（如 `musica/`、`patch/`、`staging/`、`backup/<id>/`、`patch-result.json`），不跟随目录链接，也不污染原商业目录。
- **应用图标**：Windows、macOS、iOS、Android 和 Linux 安装包使用 `Assets/Branding/shinkiemu-icon.svg`。Android 自适应图标的前景和背景分开绘制，OpenHarmony 仍使用独立的图标资源。详见[品牌规范](../../Assets/Branding/BRANDING.md)。

## 维护者打包与发布

完整的依赖锁定细节、Ren'Py SDK 准备、Apple 交叉构建、OpenHarmony HAP 打包与托管交接 gate、对应源码与许可收集规则、第三方核心身份哈希算法以及历史版本交付记录，请阅读[发布与维护手册](release-runbook.md)。

## 打包与发布

参见维护手册中的[打包与发布](release-runbook.md#打包与发布)。
