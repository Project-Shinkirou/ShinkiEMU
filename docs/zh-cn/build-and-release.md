---
title: 构建与发布
description: Cargo 编排、核心构建脚本、打包及明确的 blocked 状态。
---

# 构建与发布

## 输入与依赖

克隆仓库时递归取得子模块，并准备 Rust/Cargo 和目标平台的原生工具链。Windows 原生构建需要 x64 MSVC 开发者环境、CMake、Ninja 和 Git。Android 使用 JDK 27、NDK 30.0.16248370 和 cargo-ndk 4.1.2。工具版本与下载输入在 `build/dependencies.toml`；核心 manifest、features、许可在 `build/cores.toml`；目标与 runner 定义在 `build/targets.toml`。

Ren’Py 另需固定的官方 SDK，设置 `RENPY_SDK_ROOT`、`RENPY_PYTHON_DEV`、`RENPY_SDL_INCLUDE`，并准备与 CPython 3.12.7 匹配的官方 Cython 3.2.4 编译器。这些属于上游编译和运行依赖。SDK 输入以 fork 的 `shinki/sdk.json` 为准。

工作区统一使用 Rust 2024 edition，最低 Rust 版本为 1.99，固定工具链为 Rust 1.99.1。Android 打包使用 Gradle 9.8.2、AGP 9.4.1 内置 Kotlin、JDK 27、platform 37.2、build-tools 37.0.0 和稳定版 NDK r30（30.0.16248370）。CMVS 正在独立工作树开发，暂不参与依赖升级后的构建；开发完成后单独合入。所有目标均标为 blocked，工作区不包含该 crate。

原生库固定 vcpkg baseline `0c2a9ace1964b818b15f7ae50ba1c42404918a7b`，对应 FFmpeg 9.0.2#2、libass 0.17.5、HarfBuzz 14.6.0、FriBidi 1.0.17、FreeType 2.14.3。CI 使用 NASM 3.02，DXC 固定为 1.9.2609。构建运行在 JDK 27 上，Java/Kotlin 字节码仍以 JVM 17 为目标，保持 Android 兼容性。RoukakuEngine 固定到已发布 revision `3598c63cea7d952d31499c94b1577233d370dc26`，外部核心来源由独立任务合入。Bangumi/VNDB SDK 的公开 API 仍要求 reqwest 0.12 类型，因此保留最新补丁版 0.12.28，应用自己的 HTTP 请求使用 0.13.5。Ren’Py 的 CPython/Cython/SDL 输入继续匹配其固定 SDK 的 ABI。Android 静态核心通过来源 patch 共用工作区的 ABI 与构建辅助库，不再解析第二份同版本的构建辅助包。

mlua 0.12.2 的 mlua-sys 0.13.0 固定到官方上游修复 `ff96ed786295cae96f43ef1372bd22be0fbf7866`。发布版在 `Lua::new` 中会拒绝当前 Luau 744 生成的字节码；上游修复将识别上限从 11 同步为 14。

Windows 交叉构建使用固定 Engine revision 中的 Android triplet，通过 Windows 扩展绝对路径前缀同时满足 Meson 1.12 的 POSIX 目标路径检查和本机暂存要求。Java 27 的运行支持见 [Gradle 9.8.2 兼容矩阵](https://docs.gradle.org/9.8.2/userguide/compatibility.html)；[Android JDK 指南](https://developer.android.com/build/jdks) 要求 JDK 不低于 AGP 的最低版本（AGP 9.4 为 17）。

从仓库根目录执行：

```sh
cargo xtask --help
cargo xtask matrix --json
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask doctor --target x86_64-pc-windows-msvc --core renpy
```

`doctor` 检查缺失输入，失败时返回非零，不安装依赖。`deps` 是显式下载和准备入口，可用 `--core <plugin_id>` 缩小范围；它准备锁定的原生依赖，不安装系统包。普通命令不会隐式执行 deps，Cargo 仍需能取得已解析的依赖。

## 构建、检查与测试

```sh
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask build --target x86_64-pc-windows-msvc --all-cores --release
cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
```

build 必须在 `--manager`、`--core`、`--all-cores` 中选一项。check 选择 Manager 或单个核心；test 在宿主运行，没有 `--target` 参数。不带选择的 test 运行 xtask 和构建辅助库测试。原生行为回归可能需要真实 GPU、音频设备及公开夹具。

每个核心的 `build.rs` 校验本核心的目标和输入，在 Cargo `OUT_DIR` 写入 `core-build.json`。C++ 适配通过它驱动真实 CMake 构建，纯 Rust 核心仍由 Cargo 编译。构建脚本不递归运行 Cargo，也不发布 Release。xtask 从 `compiler-artifact` 和 `build-script-executed` JSON 读取真实产物，不猜测 target 路径。各组件使用 `.work/target/<target>/<component>`，构建记录写入 `.work/build/`。Artemis 的 Lua 5.1 与 Manager 的 Luau 依赖图保持独立，不执行混合全工作区的 all-features 构建。

Manager 在创建网络客户端和工作线程前，仅初始化一次进程级 Rustls 加密实现，选择与 reqwest 0.13 和平台证书校验栈一致的 AWS-LC。VNDB 当前使用的 reqwest 0.12 还会启用 ring，不能依靠 feature 自动判断默认实现。Steam 等 provider 自身不安装进程级默认实现。

### Linux 桌面

使用仓库固定的 Rust 1.99.1 工具链。在 Ubuntu 22.04/24.04 上显式安装原生开发包：

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm patchelf autoconf automake libtool bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev mesa-vulkan-drivers xvfb xauth unzip jq
sudo apt-get install -y --no-install-recommends autoconf-archive libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav
cargo xtask deps --target x86_64-unknown-linux-gnu
cargo xtask deps --target x86_64-unknown-linux-gnu --all-cores
cargo xtask build --target x86_64-unknown-linux-gnu --manager --release
cargo xtask build --target x86_64-unknown-linux-gnu --all-cores --release
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-beta.1 --out .work/linux-manager
cargo xtask package --target x86_64-unknown-linux-gnu --core siglus_rs --version 0.1.0-beta.1 --out .work/linux-siglus
cargo xtask ci --target x86_64-unknown-linux-gnu --component manager --report .work/linux-ci.json
```

Linux Manager 的 `deps` 检查发行版提供的开发库，不调用 `apt`。核心的 `deps` 在各目标独立的目录中准备锁定的 vcpkg 原生库，并生成可重定位的共享库普通文件供打包使用。在打包目录中运行 `./shinki-emu-manager`。打包会检查 ELF 架构、加载路径和依赖闭包，收集许可证及离线元数据快照；`ci` 报告的 ZIP 保留执行权限。CI 与发布任务使用同一组依赖和 xtask 命令。核心可用性仍以 `matrix --json` 为准，声明为 blocked 的核心不产生二进制附件。
Linux 包使用发行版提供的 glibc、libstdc++、OpenSSL 3、fontconfig、udev 和 ALSA ABI 库。目标系统需安装对应运行包，并提供带图形驱动的 X11 或 Wayland 桌面。

RFVP 还依赖发行版提供的 GStreamer 1.x（至少 1.14）、GLib 2.x（至少 2.56）及 base、good、libav 插件，用于原生影片解码。这些库作为系统运行要求声明，不随核心复制。在 Ubuntu 上安装 `libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav`。包内 `build.json` 的 `system_dependencies` 记录实际依赖的系统 SONAME。

下载不稳定时，可将固定版本的源码和工具压缩包预先放入 `.work/vcpkg-downloads`，用可续传分块或 `rsync --partial` 传输。vcpkg 仍会按实际选中的版本化 port 校验每个压缩包的 SHA512。不要用发行版的 libass/FFmpeg 替换锁定的原生构建。

`deps` 默认拒绝有修改的 fork 工作区。调试未提交的 adapter 改动时，可显式传入 `--allow-dirty-sources`；工作区 HEAD 必须已是清单固定的 revision，源码准备不会对该工作区执行 fetch、checkout 或子模块更新。构建记录保留源码修改状态，`release` 拒绝有修改的源码及其构建记录。

Linux CI 与发布任务还会解压报告中的 Manager ZIP，使用独立用户数据目录在 Xvfb 下启动。进程必须保持运行 15 秒，提前退出会使任务失败。

### Android Manager 测试 APK

使用 JDK 27（设置 `JAVA_HOME`，将其 `bin` 加入 `PATH`）、Android SDK platform 37.2、build-tools 37.0.0、NDK 30.0.16248370 和 cargo-ndk 4.1.2。`ANDROID_HOME` 指向 SDK 根目录，`ANDROID_NDK_HOME` 指向其中的 `ndk/30.0.16248370`。构建只接受清单固定的稳定版 NDK revision。缺少 SDK 包时，显式调用该 SDK 的 `sdkmanager --sdk_root=<sdk> --channel=0 "platforms;android-37.2" "build-tools;37.0.0" "ndk;30.0.16248370"`。将 `CARGO_HOME`、`CARGO_TARGET_DIR`、`TEMP`/`TMP` 和 `GRADLE_USER_HOME` 放在空间充足的磁盘上。

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo xtask deps --target aarch64-linux-android
cargo xtask deps --target x86_64-linux-android
cargo xtask build --target aarch64-linux-android --manager --release
cargo xtask build --target x86_64-linux-android --manager --release
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.2 --out .work/android-test
```

每次构建都以 `cargo ndk --locked` 编译，静态链接 RFVP，并在构建记录所指 target 目录的 `android-inputs/` 中收集 JNI 运行依赖、元数据和许可。打包需要**两个三元组**来自同一源码提交的 release 构建记录。Gradle 随后生成含 `arm64-v8a`、`x86_64` 和 `assets/metadata/` 的通用 APK；xtask 检查签名及每个原生库的依赖闭包。没有签名密钥时，输出为 `ShinkiEMU-<version>-android-universal-debug-signed.apk`，这是 debug 签名的 CI 测试包，不是正式安装包。测试密钥生成在 `GRADLE_USER_HOME` 中。证书验证器的 Kotlin 组件由 Gradle 从官方 Maven archive 获取，版本与 `Cargo.lock` 中的 Android shim 同步，不再从 Cargo 源码目录复制 AAR。

首次下载 Gradle 依赖后，可设置 `SHINKI_EMU_GRADLE_OFFLINE=1` 传入 `--offline`；每次打包都使用新的输出目录。`cargo xtask ci --target aarch64-linux-android --component manager --report .work/android-ci.json` 构建两个 ABI，并报告实际 APK 路径。CI 会先为两个三元组准备依赖，再运行同一命令。

运行限制：原版 Slint 1.18.1 在 SwiftShader/软件 Vulkan 模拟器上存在只渲染首帧、后续界面不再更新的问题；请使用 `-gpu host` 或实体设备。独立的单 Text、单 Timer 应用在五次冷启动中均复现了更新停滞；同一 Manager 测试 APK 在主机 GPU 模式下五次冷启动均正常显示中文引导及图标。构建、打包成功不代表游戏兼容性已验证。

### Apple Manager：原生 macOS 与 Linux 交叉构建

原生 CI 继续使用 `build/targets.toml` 中的 macOS runner，通过 Cargo/Xcode 构建。在 Linux 上，`build`、`check` 自动选择 cross 0.2.5 和 `Cross.toml` 配置的私有镜像。Apple 目标包括 `x86_64-apple-darwin`、`aarch64-apple-darwin`、设备目标 `aarch64-apple-ios`，以及模拟器目标 `aarch64-apple-ios-sim`、`x86_64-apple-ios`。最低系统版本为 macOS 13.0、iOS 16.0。

从 [Mac App Store](https://apps.apple.com/app/xcode/id497799835) 或 [Apple Developer Downloads](https://developer.apple.com/download/all/) 取得 Xcode，遵守 Apple 许可。在 Mac 上用 `xip --expand` 展开官方 Xcode `.xip`，选择这套 Xcode，并在该 Mac 上接受许可。单独的 Command Line Tools 不提供 iOS SDK。随后执行：

```sh
for sdk in macosx iphoneos iphonesimulator; do
  tar -C "$(xcrun --sdk "$sdk" --show-sdk-path)" -cJf "$sdk.sdk.tar.xz" .
done
```

私下传输归档，在 Linux 上分别解压。`SHINKI_APPLE_SDK_DIR` 直接指向含 `SDKSettings.json`、`System/`、`usr/` 的 SDK 内容目录，不能指向父目录或归档。Darwin 使用 `macosx`，设备使用 `iphoneos`，两个模拟器使用 `iphonesimulator`。流水线不从镜像站下载 SDK。SDK 和含 SDK 的镜像不得进入 Git 或公共镜像仓库。

把源码、`CARGO_HOME`、`RUSTUP_HOME`、`CARGO_TARGET_DIR` 和工具都放在专用目录，例如 `~/shinkirou-apple/`。显式安装 Rust 1.99.1 及所选 Rust target，再通过 `cargo +1.99.1 install --locked --root "$HOME/shinkirou-apple/tools"` 安装 cross 0.2.5 和 apple-codesign 0.29.0（提供 `rcodesign`），将工具目录的 `bin` 加入 `PATH`。创建镜像要求 `/` 至少剩余 8 GiB，不会删除用户文件或镜像。在本仓库执行：

Docker 需要 Buildx/BuildKit（用 `docker buildx version` 检查）。私有插件可放在 `$DOCKER_CONFIG/cli-plugins/docker-buildx`，无需 sudo，也不改用户原有 Docker 配置。[Buildx v0.38.1 官方 Linux x86_64 附件](https://github.com/docker/buildx/releases/download/v0.38.1/buildx-v0.38.1.linux-amd64) 的 SHA256 为 `e25d5cd675092bb5d1d03632fdc46cdf3fede7b514dc5f023a105e00d3ce5ad4`，核对后再赋予执行权限。

```sh
export DOCKER_CONFIG="$HOME/shinkirou-apple/docker"
export SHINKI_APPLE_SDK_DIR="$HOME/shinkirou-apple/sdks/macosx.sdk"
cargo xtask apple-image --target aarch64-apple-darwin
cargo xtask deps --target aarch64-apple-darwin
cargo xtask build --target aarch64-apple-darwin --manager --release
cargo xtask package --target aarch64-apple-darwin --manager --version 0.1.0-beta.1 --out .work/macos-manager
export SHINKI_APPLE_SDK_DIR="$HOME/shinkirou-apple/sdks/iphonesimulator.sdk"
cargo xtask apple-image --target aarch64-apple-ios-sim
cargo xtask build --target aarch64-apple-ios-sim --manager --release
cargo xtask package --target aarch64-apple-ios-sim --manager --version 0.1.0-beta.1 --out .work/ios-simulator-manager
```

每个目标单独准备镜像并选择对应 SDK。镜像固定 Rust、osxcross 和 vcpkg 输入，提供 LLVM/LLD、C/C++、CMake 和 Apple triplet；宿主编译器与目标编译器分开配置。原生依赖许可导出到 `.work/apple-native/<target>`，镜像身份相同时才复用。缺 SDK、镜像或 Rust target 时 `doctor` 返回错误；支持目标的真实编译错误仍然按失败处理。

macOS 包含真实的 `ShinkiEMU-adhoc.app`，可执行文件位于 `Contents/MacOS`，元数据和许可位于 `Contents/Resources`，`ci`/`release` 生成 ZIP。iOS Manager 在主线程使用 Slint 的 winit/UIKit 后端和 Metal 渲染器，原生 Files 选择器保留安全作用域授权及书签，应用数据存入自身 Library 沙箱。iOS 重启需由用户关闭再打开应用。共享平台会激活真实的 AVAudioSession；Manager 编译成功不代表可以在 iOS 安装任意下载的核心二进制，核心自身的嵌入与构建限制仍在清单中。

模拟器包包含 `simulator-test.app`；设备打包生成带 `Payload/*.app` 的 `adhoc-device-test.ipa`，`ci` 报告该 IPA 的真实路径。打包先校验单架构 Mach-O、设备/模拟器平台以及系统/随包依赖闭包，再签署副本。Linux 使用 `rcodesign sign` 并逐个验证 Mach-O，原生 macOS 使用 `codesign`。ad-hoc 签名是真实签名，但不提供 Apple provisioning、公证或 App Store 身份。设备安装仍需有效的 Apple 签名及 provisioning，这些产物用于测试与集成，不是正式安装包。Release 名称包含 iOS 的设备/模拟器 variant，避免同架构附件重名。

SDK 验收需保留真实产物路径、大小，以及 `file`、`llvm-objdump --macho --dylibs-used --private-headers <executable>`、`rcodesign verify <executable>` 的输出。缺少合法 SDK 时，这项验收仍受阻；编译成功本身不代表游戏运行兼容。

## 矩阵与 CI

`matrix --json` 从权威清单生成全部核心与 Manager 在已声明目标上的组合，包括两种 iOS 模拟器架构。当前组合和原因见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，文档不另抄一份支持状态表。Android 中静态集成 RFVP 的 Manager 与独立动态核心包是两条不同交付路径。

```sh
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

清单中未适配的组合报告 `blocked`、具体原因和 `artifact: null`。Apple 核心行保留源码中真实的嵌入及原生依赖限制；OpenHarmony 需要原生宿主适配和 `OHOS_SDK_NATIVE`。清单声称支持的组合编译失败，报告 `failed` 并返回非零。`--all-cores` 跳过已声明的 blocked 组合，但不掩盖支持组合的失败。`publish_blocked` 单独记录分发限制，不等同于无法编译。

## 打包与发布

先构建所选组件，再打包构建记录指向的产物：

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --version 0.1.0-beta.1 --out .work/packages/rfvp
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/packages/manager
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc --version 0.1.0-beta.1
```

输出目录必须尚不存在。打包拒绝不安全路径、链接、冲突、缺许可、架构错误、缺 query 导出和原生依赖不完整的输入。核心包包含库、资源、依赖、许可证、构建来源及带哈希的 `core.json`；源码包保留适用的对应源码与重链接输入。

Manager 打包会收集经目标平台筛选的 Cargo 依赖闭包（普通依赖和构建依赖，不含仅供测试的依赖边）。各 crate 声明的 `license_file`、源码根目录中的 `LICENSE*`、`COPYING*`、`NOTICE*` 文件及同类许可目录的内容，放在 `licenses/rust/<名称>-<版本>/`，同目录的 `package.json` 记录 SPDX 表达式和文本来源。crate 未提供文件时，打包器从固定的 `spdx` 0.13.6 内置离线数据库中，取得 SPDX License List 3.29.0 的标准文本，将表达式引用的全部许可和例外条款写为 `LICENSE-<SPDX>`。来源版本与压缩包 SHA256 记录在 `build/dependencies.toml`，并与 Cargo 固定依赖核对；打包时不下载许可文本。无法识别的回补标识符，或既无许可文件也无表达式的 crate，都会使打包失败。Material 组件、图标及原生依赖的许可单独收集。

源码许可收集也覆盖嵌套资源的许可文件和字体同名文本说明，保留相对路径，不复制字体本身。Git 工作区成员的包目录没有许可文件时，会收集该检出根目录提供的许可文件，先保留上游版权声明，再考虑使用 SPDX 标准文本。

Release 标签为 `manager-v<semver>` 或 `core-<slug>-v<semver>`。slug 将 plugin_id 转为小写，并把点和下划线替换为连字符，例如 `ReMinori` 对应 `reminori`；plugin_id 本身不变。不带 `--publish` 时只准备本地产物。发布需要 `--publish` 及 `GH_TOKEN` 或 `GITHUB_TOKEN`；只上传真实成功的平台产物，拒绝空 Release 和同名不同内容的附件。核心发布在核对远端字节后，向 `Metadata/engines/cores/` 提议目录更新 PR。Manager 与核心独立发布；标签不会绕过分发限制，也不会把 Android debug 签名测试包变成正式安装包。

元数据命令见 [Metadata](metadata.md)，核心包契约见[核心 ABI](core-abi.md)。
