---
title: 构建与发布
description: Cargo 编排、核心构建脚本、打包及明确的 blocked 状态。
---

# 构建与发布

## 输入与依赖

克隆仓库时递归取得子模块，并准备 Rust/Cargo 和目标平台的原生工具链。Windows 原生构建需要 x64 MSVC 开发者环境、CMake、Ninja 和 Git。Android 使用 JDK 17、NDK 30.0.15729638 和 cargo-ndk 4.1.2。工具版本与下载输入在 `build/dependencies.toml`；核心 manifest、features、许可在 `build/cores.toml`；目标与 runner 定义在 `build/targets.toml`。

Ren’Py 另需固定的官方 SDK，设置 `RENPY_SDK_ROOT`、`RENPY_PYTHON_DEV`、`RENPY_SDL_INCLUDE`，并准备与 CPython 3.12.7 匹配的官方 Cython 3.2.4 编译器。这些属于上游编译和运行依赖。SDK 输入以 fork 的 `shinki/sdk.json` 为准。

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

## 矩阵与 CI

`matrix --json` 从权威清单生成全部核心与 Manager 在九个目标三元组上的组合。当前组合和原因见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，文档不另抄一份支持状态表。Android 中静态集成 RFVP 的 Manager 与独立动态核心包是两条不同交付路径。

```sh
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

清单中未适配的组合报告 `blocked`、原因和 `artifact: null`。iOS/OpenHarmony 缺少原生宿主时，安装 SDK 也不会补齐适配；OpenHarmony 还需 `OHOS_SDK_NATIVE`。清单声称支持的组合编译失败，报告 `failed` 并返回非零。`--all-cores` 跳过已声明的 blocked 组合，但不掩盖支持组合的失败。`publish_blocked` 单独记录分发限制，不等同于无法编译。

## 打包与发布

先构建所选组件，再打包构建记录指向的产物：

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --version 0.1.0-beta.1 --out .work/packages/rfvp
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/packages/manager
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc --version 0.1.0-beta.1
```

输出目录必须尚不存在。打包拒绝不安全路径、链接、冲突、缺许可、架构错误、缺 query 导出和原生依赖不完整的输入。核心包包含库、资源、依赖、许可证、构建来源及带哈希的 `core.json`；源码包保留适用的对应源码与重链接输入。

Release 标签为 `manager-v<semver>` 或 `core-<slug>-v<semver>`。slug 将 plugin_id 转为小写，并把点和下划线替换为连字符，例如 `ReMinori` 对应 `reminori`；plugin_id 本身不变。不带 `--publish` 时只准备本地产物。发布需要 `--publish` 及 `GH_TOKEN` 或 `GITHUB_TOKEN`；只上传真实成功的平台产物，拒绝空 Release 和同名不同内容的附件。核心发布在核对远端字节后，向 `Metadata/engines/cores/` 提议目录更新 PR。Manager 与核心独立发布；标签不会绕过分发限制，也不会把 Android debug 签名测试包变成正式安装包。

元数据命令见 [Metadata](metadata.md)，核心包契约见[核心 ABI](core-abi.md)。
