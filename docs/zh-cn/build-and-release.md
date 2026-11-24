---
title: 构建与发布
description: Cargo 编排、核心构建脚本、打包及明确的 blocked 状态。
---

# 构建与发布

在仓库根目录用 Cargo 构建 Manager 和独立核心。查看目标矩阵、打包、发布和处理元数据用 `cargo xtask`。输入回放、游戏帧和界面截图的检查方法见[无窗口运行](headless.md)。

## 游戏目录中的磁盘布局

ShinkiEMU 自己写入游戏目录的文件统一放在 `<game>/.shinki/` 下。Musica 存档用 `musica/saves/`，下载的补丁用 `patch/`，事务暂存用 `staging/`，原文件和清单用 `backup/<id>/`，原位覆盖的结果写进 `patch-result.json`。写入时校验路径，不跟随目录链接。第三方核心沿用各自上游的存储约定。

## 输入与依赖

用 `git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU` 克隆。已有工作副本时，运行 `git submodule update --init --recursive` 初始化工作区使用的 `ThirdParty/cpal` 补丁。安装 Git 和 rustup，由 [`rust-toolchain.toml`](../../rust-toolchain.toml) 选用 Rust 1.99.1。按需安装 Rust 目标和对应的原生编译器、CMake、Ninja、SDK。

构建脚本会在 `.work/vcpkg` 准备固定版本的 vcpkg，按清单和 triplet 安装到 `.work/vcpkg-installs/<triplet>-<key>/installed/`。下载和二进制缓存分别在 `.work/vcpkg-downloads/`、`.work/vcpkg-binary-cache/`。如果要共用一份固定版本的 vcpkg，设置 `SHINKI_EMU_VCPKG_ROOT`。系统包和 SDK 仍要单独安装。KiriKiri 的解析器生成工具在构建主机上运行：Windows 的构建脚本可获取固定版 WinFlexBison，其他主机需要安装 Bison，也可用 `BISON_EXECUTABLE` 指定路径。KrKr2 要求 Bison 3.8.2 或更新版本。

打包、运行 `ci` 或准备发布前，先安装固定版本的许可证工具：

```sh
cargo install cargo-about --locked --version 0.9.3
```

### Windows x64

安装 Visual Studio C++ 工具、CMake 和 Ninja。在 x64 Visual Studio 开发者环境中运行 Cargo 和打包命令（`vcvars64.bat`，PowerShell 用 `Launch-VsDevShell.ps1 -Arch amd64`）。打包从 `%VCToolsRedistDir%\x64` 取 `VCRUNTIME140.dll` 等可再分发的 MSVC 运行库。

### Linux 桌面

Ubuntu 24.04 上的桌面构建需要以下开发包：

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm yasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev libxinerama-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev libgtk-3-dev libwebkit2gtk-4.1-dev unzip
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev
```

RFVP 通过 `pkg-config` 找到 GStreamer 和 GLib，核心 ZIP 不包含这些库。在运行 Ubuntu 的设备上安装 `libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0t64 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav`。尚未切换到 t64 的发行版使用 `libglib2.0-0`。

在带图形驱动的 X11 或 Wayland 环境中，从解压后的 Manager 目录运行 `./shinki-emu-manager`。Manager 使用发行版的 glibc、libstdc++、fontconfig、udev 和 ALSA，商店登录还需 GTK 3 和 WebKitGTK 4.1（Ubuntu 安装 `libwebkit2gtk-4.1-0` 会同时装上 GTK 3）。Impacto 另需 `libdrm2`。KiriKiri2 使用系统的 OpenGL、X11、OpenMP、Pango 和 GTK（Ubuntu 24.04 上为 `libgl1 libglx0 libopengl0 libx11-6 libgomp1 libpangocairo-1.0-0 libpangoft2-1.0-0 libgtk-3-0t64`）。

### Android

构建通用 Manager APK 需要 cargo-ndk 4.1.2 和两个 Rust 目标。将 `ANDROID_HOME` 指向 SDK 目录，`ANDROID_NDK_HOME` 指向其中的 `ndk/30.0.16248370`。APK 打包需要 JDK 27（设置 `JAVA_HOME`，将其 `bin` 加入 `PATH`）、SDK platform 37.2 和 build-tools 37.0.0。仓库自带 Gradle wrapper。

```sh
cargo install cargo-ndk --locked --version 4.1.2
rustup target add aarch64-linux-android x86_64-linux-android
sdkmanager --sdk_root="$ANDROID_HOME" --channel=0 "platforms;android-37.2" "build-tools;37.0.0" "ndk;30.0.16248370"
```

Android Manager 内嵌 Ren’Py。配置好 Ubuntu 24.04 x86_64 主机后，Cargo 会在 `.work/native-downloads/renpy-android/<Rust target>/<recipe key>/` 里准备目标运行库。配方需要装有 Jinja2 的 Python 3、LLVM/Clang/LLD 18、ccache 和 [CI](../../.github/workflows/ci.yml) 中列出的开发包。用非 Linux 主机构建时，把 Linux 生成的对应 ABI 的完整运行库目录复制过去，将 `RENPY_STATIC_ROOT` 指向它。目录应包含 `include/`、`lib/`、`Resources/` 和许可声明；换 ABI 时也要换目录。Ren’Py 配方用独立的 Linux NDK r29，ShinkiEMU 的 Cargo 构建用上面的 NDK r30。配方输入见 [Ren’Py](cores/renpy.md)。

### Apple

在 macOS 上安装 Xcode，用 `xcode-select` 选中并接受许可。最低部署版本为 macOS 13、iOS 16。macOS 安装 `x86_64-apple-darwin` 或 `aarch64-apple-darwin`，iOS 实机安装 `aarch64-apple-ios`，模拟器安装 `aarch64-apple-ios-sim` 或 `x86_64-apple-ios`。

在 Linux 上交叉构建需要带 Buildx 的 Docker、cross 0.2.5、apple-codesign 0.29.0（`rcodesign`）。在安装了 [Xcode](https://developer.apple.com/download/all/) 的 macOS 上，用 `xcrun --sdk <sdk> --show-sdk-path` 找到目标的 `macosx`、`iphoneos` 或 `iphonesimulator` SDK，再私下传输。`SHINKI_APPLE_SDK_DIR` 指向解压后含 `SDKSettings.json`、`System/` 和 `usr/` 的目录。按目标生成私有镜像，不公开 SDK 或含 SDK 的镜像。

```sh
cargo xtask apple-image --target aarch64-apple-darwin
cross build --locked --release --target aarch64-apple-darwin -p shinki-emu-manager
```

### OpenHarmony Manager

使用 DevEco Studio 自带的 OpenHarmony Native SDK 5.0.5（API 17）。安装 Rust 目标 `aarch64-unknown-linux-ohos`，将 `OHOS_SDK_NATIVE` 指向 `sdk/default/openharmony/native`。打包 HAP 时，将 `SHINKI_EMU_OHOS_SIGNING_DIR` 设为仓库外的目录。打包工具会配置 SDK 链接器，生成用 SDK 测试 PKI 签名的 HAP。这个 HAP 用于实验，尚未验证在零售设备上的安装或真机启动。

## 构建、检查与测试

Manager 的包名是 `shinki-emu-manager`，独立核心选择 [`Source/Cores/`](../../Source/Cores/) 下的入口包。包名以 `shinki-emu-core-` 开头，核心 ID 中的点换成连字符，保留大小写和下划线。例如 `rfvp` 对应 `shinki-emu-core-rfvp`，`shinki.emu.musica` 对应 `shinki-emu-core-shinki-emu-musica`。

```sh
cargo build --locked --release -p shinki-emu-manager
cargo build --locked --release -p shinki-emu-core-shinki-emu-musica
cargo check --locked -p shinki-emu-core-rfvp
cargo test --locked -p shinki-emu-musica
cargo test --locked -p xtask -p shinki-emu-build
```

要指定原生目标，传入 `--target <triple>`。宿主测试直接选实现相应行为的 crate。本地构建通过 Cargo feature 选择变体，例如给 Manager 加 `--features no-core-download`。可用 `SHINKI_EMU_PRODUCT_VERSION` 指定 Manager 的产品版本，未指定时使用 Manager 的包版本。

Android 用 cargo-ndk 指定 API 28。下面分别构建 ARM64 Manager 和 x86_64 独立 Siglus 核心：

```sh
cargo ndk -t arm64-v8a -P 28 build --locked --release -p shinki-emu-manager --lib
cargo ndk -t x86_64 -P 28 build --locked --release -p shinki-emu-core-siglus_rs
```

独立核心的 `supported_targets`、Manager 的 `static_manager_targets` 及各目标不支持的原因都在 [`build/cores.toml`](../../build/cores.toml) 中。能构建不等于游戏已验证可玩。Android 独立核心必须有经过验证的 `android_dynamic` 策略才可安装。iOS 使用随应用提供的核心，OHOS 内置 Musica，两者都不下载安装核心。`publish_blocked` 单独限制分发，不改变构建支持范围。

### 编译进程序的商店插件

Manager 默认启用 `store-steam`、`store-dlsite` 和 `store-dmm`。只启用 Steam 时，给 Cargo 传 `--no-default-features --features store-steam`。要接入新商店，添加实现 `shinki-emu-store` trait 的 crate，接上可选依赖和 feature，再到 `manager_app/store.rs` 注册。商店插件返回不透明的计划、不含凭据的固定错误和账号拥有的作品。临时文件写进传入的 `.shinki/store/<provider>/` 目录，凭据通过 `SecretStore` 保存。

## 新增核心或更新固定提交

1. 参考最接近的现有实现，在 `Source/Cores/<id>/` 添加入口 crate。自研 Rust 核心通过路径依赖引用实现，由入口 `cdylib` 导出 ABI query；Rust fork 继承工作区依赖，按需开启独立构建用的导出 feature。原生 adapter 由构建脚本把原生库交给打包工具。静态嵌入 Manager 时，按目标为实现依赖开启静态 feature，并在 `static_cores.rs` 注册 Rust query。
2. 在根目录 [`Cargo.toml`](../../Cargo.toml) 的 `[workspace.dependencies]` 里新增或更新 fork 的 `git` URL 和完整 `rev`。`build/cores.toml` 管理产品元数据，包括 ID、许可和声明、目标支持、静态嵌入及外部核心的上游身份与发布序号。`adapter_licenses` 的路径相对于 fork 根目录，`licenses` 的路径相对于 ShinkiEMU 根目录。
3. 不加 `--locked` 运行一次 `cargo metadata --format-version 1`，更新 [`Cargo.lock`](../../Cargo.lock)。检查差异是否与本次新增或调整的提交一致，不批量更新无关依赖。将 manifest 和锁文件一起提交。日常构建使用 `--locked`。

**每个外部核心的公开源码就是 `Cargo.lock` 记录的公开 fork 提交。** 发包前要确认公众可以获取该提交。自研核心的源码在 ShinkiEMU 的发布提交中。

### 在本地修改 fork

把本地 fork 放在仓库外，在工作区根目录的 `Cargo.toml` 临时增加 patch。表名用原始 Git URL，路径指向本地 fork 中的 adapter crate。以 RFVP 为例：

```toml
[patch."https://github.com/Project-Shinkirou/rfvp"]
shinki-emu-rfvp = { path = "../rfvp/crates/shinki-emu-rfvp" }
```

使用 fork 的实际包名和兼容版本。运行 `cargo metadata --format-version 1` 解析 patch，然后照常构建入口包。提交发布改动前移除这个 patch，将锁文件恢复为公开 Git 提交。

## 矩阵与 CI

```sh
cargo xtask matrix
cargo xtask matrix --json --ci --since origin/main
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

`matrix` 列出已声明的组合及阻塞原因。JSON 只列可在托管 runner 上构建的行。ARM64 行负责生成通用 APK 时，`--ci` 会去掉重复的 Android x86_64 Manager 行。`--since` 只保留受改动影响的行。`ci` 打包所选目标，报告产物或阻塞原因。OpenHarmony 在装有 DevEco 的本地机器上处理。

## 打包与发布

`package` 会运行锁定依赖的 release 构建，将 Cargo 报告的库、资源和原生运行依赖放进新的输出目录。运行前先安装 `cargo-about`。核心包保留 ABI v3 的 `core.json` 和文件哈希。包内包含 `cargo-about` 生成的 Rust 许可声明、明确列出的核心与原生依赖声明，以及适用的 Manager 离线元数据。仍需遵守第三方许可和重链接义务。

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/manager-package
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.1 --out .work/android-test
```

常规 Android Manager 包会将两个 ABI 打进同一个通用 APK，请在配好环境的 Linux 主机上打包。设置 `GRADLE_USER_HOME` 存放调试密钥。不提供发布签名材料时，生成调试签名的测试 APK。只需本地测试单个 ABI 时，可在 Manager 打包命令上加 `--qa-single-abi`，例如在非 Linux 主机上使用相应的 Ren’Py 运行库。公开 APK 必须使用项目发布证书。Android 独立核心产出核心包，无需 Gradle 签名。macOS 产出 `.app`，iOS 产出模拟器测试 `.app` 或真机测试 `.ipa`。Apple 默认使用 ad hoc 签名，真机安装需要有效的 provisioning。

打包时用 `SHINKI_EMU_MANAGER_FEATURES` 追加 Cargo feature，以逗号分隔。iOS 的 `ios-framework-cores` 和 HAP 的 `ohos-bundled-musica` 变体只用于本地 QA，不公开发布。

Manager 和自研核心发布时指定统一的产品 `--version`（SemVer）。外部核心省略此参数。它的身份由原项目的 `upstream_identity`、`Cargo.lock` 解析出的适配仓库与提交、`adapter_branch` 和该核心的 `release_sequence` 决定。上游填写真实标签或分支，以及完整提交。外部核心版本为 `<tag|branch>-<ref slug>-<digest>`：取 ref 的前 16 个字节，ASCII 字母转小写，数字保留，其余换成连字符。摘要是上游仓库、ref 类型、ref 名、上游提交、适配仓库、适配分支、适配提交及十进制序号的 SHA-256，每个字段用 UTF-8 编码并在末尾加 LF。标签格式分别为 Manager 的 `manager-v<semver>`、自研核心的 `core-<slug>-v<semver>` 和外部核心的 `core-<slug>-<version>`。标签中的 slug 将核心 ID 转成小写，点和下划线换成连字符。调整外部核心的固定提交或再次发布时递增序号，目录按序号选择最新的带身份记录。

```sh
cargo xtask release --component rfvp --dry-run
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

`--dry-run` 只列出版本、标签和目标。`--target` 构建并将产物与 `release.json` 放在 `.work/release/<tag>/<target>/`。`--publish` 使用 `GH_TOKEN` 或 `GITHUB_TOKEN` 发布准备好的产物，发布核心时还会创建目录更新 PR。发布时核对产物哈希，目录记录公开文件的 SHA-256 和大小。只发 Windows/Android 时，规划、准备和公开都传同样的 `--scope windows-android`，默认范围为 `all`。

[发布工作流](../../.github/workflows/release.yml) 准备托管平台产物。发布全平台 Manager 时，选用一次成功的 `release.yml` 运行，确保它与本地检出是同一提交，且未覆盖 `source_commit`。下载这次运行的所有 `release-files-manager@*` 产物，将其中各个 `<tag>/<target>/` 目录合并到 `.work/release/<tag>/`。在本地配置 DevEco 和签名目录，用同一产品版本制作 HAP 并发布：

```sh
cargo xtask release --component manager --version 0.1.0-beta.1 --target aarch64-unknown-linux-ohos
cargo xtask release --component manager --version 0.1.0-beta.1 --scope all --publish
```

快照和目录命令见[元数据](metadata.md)，原生包格式见[核心 ABI](core-abi.md)。
