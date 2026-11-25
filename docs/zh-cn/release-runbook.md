---
title: 发布与维护手册
description: Cargo 打包、签名、发布条件和历史交付记录。
---

# 发布与维护手册

本地构建和测试见[从源码构建](build-and-release.md)。本手册记录 Cargo 打包流程、签名、发布条件和历史交付。

[游戏目录中的磁盘布局](build-and-release.md#游戏目录中的磁盘布局)同时适用于本地构建和发行包。

## 打包与发布

`package` 会运行锁定依赖的 release 构建，将 Cargo 报告的库、资源和原生运行依赖放进新的输出目录。运行前先安装 `cargo-about`。核心包保留 ABI v3 的 `core.json` 和文件哈希。包内包含 `cargo-about` 生成的 Rust 许可声明、明确列出的核心与原生依赖声明，以及适用的 Manager 离线元数据。仍需遵守第三方许可和重链接义务。

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-qa.1 --out .work/manager-package
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-qa.1 --out .work/android-test
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-qa.1 --out .work/linux-manager

# ARM64 测试设备的本地单 ABI APK
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-qa.1 --qa-single-abi --out .work/android-arm64-test
```

QA 包用独立的 SemVer，例如上面的 `0.1.0-qa.1`。Gradle 首次同步成功后，可设置 `SHINKI_EMU_GRADLE_OFFLINE=1` 离线打包。打包器会把 `--version` 传入本次构建，不需要先生成构建回执。

常规 Android Manager 打包会将两个 ABI 放进同一个通用 APK。设置 `GRADLE_USER_HOME` 存放调试密钥。不提供发布签名材料时，生成调试签名的测试 APK。本地只测一个 ABI 时，加 `--qa-single-abi`。APK 内置 Impacto 和 KiriKiri 的 Java 宿主；其他获准的核心单独打包，由 Manager 安装。独立核心包不需要 Gradle 签名。

制作正式 APK 时，`cargo xtask package` 核对正式签名证书、包名 `org.shinkirou.shinkiemu`、指定的 `versionName` 和 `versionCode`，以及 `minSdk` 28。`release.yml` 用 `github.run_number` 设置 `SHINKI_EMU_ANDROID_VERSION_CODE`，每次公开的 APK 的 `versionCode` 都必须高于上一版。保留现有工作流文件，不要重建、改名或用另一个文件替代，否则编号可能从 1 重新开始，已安装的用户无法升级。

macOS 产出 `.app`，iOS 产出模拟器测试 `.app` 或真机测试 `.ipa`。Apple 默认使用 ad hoc 签名，真机安装需要有效的 provisioning。

打包时用 `SHINKI_EMU_MANAGER_FEATURES` 追加 Cargo feature，以逗号分隔。iOS 的 `ios-framework-cores` 和 HAP 的 `ohos-bundled-musica` 变体只用于本地 QA，不公开发布。

打包或发布 Manager、自研核心时，用 `--version` 指定产品的 SemVer；外部核心省略此参数。

外部核心的身份由原项目的 `upstream_identity`、`Cargo.lock` 解析出的适配仓库与提交、`adapter_branch` 和该核心的 `release_sequence` 决定。上游填写真实标签或分支，以及完整提交。外部核心版本为 `<tag|branch>-<ref slug>-<digest>`：取 ref 的前 16 个字节，ASCII 字母转小写，数字保留，其余换成连字符。摘要是上游仓库、ref 类型、ref 名、上游提交、适配仓库、适配分支、适配提交及十进制序号的 SHA-256，每个字段用 UTF-8 编码并在末尾加 LF。标签格式分别为 Manager 的 `manager-v<semver>`、自研核心的 `core-<slug>-v<semver>` 和外部核心的 `core-<slug>-<version>`。标签中的 slug 将核心 ID 转成小写，点和下划线换成连字符。调整外部核心的固定提交或再次发布时递增序号，目录按序号选择最新的带身份记录。

```sh
cargo xtask release --component rfvp --dry-run
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

`--dry-run` 不构建产品，只列出版本、标签、构建目标和各目标已知的 `publish_blocked` 原因。`--target` 构建并将产物与 `release.json` 放在 `.work/release/<tag>/<target>/`。`--publish` 使用 `GH_TOKEN` 或 `GITHUB_TOKEN` 发布准备好的产物，发布核心时还会创建目录更新 PR。发布时核对产物哈希，目录记录公开文件的 SHA-256 和大小。只发 Windows/Android 时，规划、准备和公开都传同样的 `--scope windows-android`，默认范围为 `all`。

每份发布记录都要标明本次发布的产品提交。记录来自其他提交，或未记录提交时，需要重新打包。

公开分发前，检查 `licenses/pinned-sources.txt` 中的确切提交链接，并按实际分发的二进制文件核对对应源码和重链接材料。独立 Ren’Py 核心和内嵌 Ren’Py 的 Android Manager APK 只有满足 [Ren’Py 源码与重链接要求](../licenses/renpy/README.md)后才能发布。仍可在本地打包；发布计划不代表可以公开分发。

[发布工作流](../../.github/workflows/release.yml) 准备托管平台产物。手动触发时传入 `components`，Manager 和选中的自研核心使用同一个产品 `version`。只发 Windows/Android 时设置 `release_scope=windows-android`，只准备产物而不发布时设置 `prepare_only=true`。

仅准备产物时，可以用 `source_commit` 指定确切的 40 位产品提交。此参数只在 `prepare_only=true` 时有效。指定后，触发工作流的 SHA 与产品 SHA 分开记录。

发布全平台 Manager 时，选一次成功且未覆盖 `source_commit` 的运行，将运行编号设为 `RUN_ID`。确认下面两条命令输出的提交一致，再下载产物：

```sh
gh run view "$RUN_ID" --json headSha --jq .headSha
git rev-parse HEAD
gh run download "$RUN_ID" --pattern 'release-files-manager@*' --dir .work/release
```

所有受支持的目标都必须有可发布的包。调试签名的 Android APK 不能替代正式包。

下载会为每份产物创建一个目录。将其中的 `<tag>/<target>/` 合并到 `.work/release/<tag>/`。配置好 DevEco、签名目录和 `cargo-about`，在同一份工作副本中制作 HAP 并发布。`VERSION` 设为该次运行的产品版本：

```sh
cargo xtask release --component manager --target aarch64-unknown-linux-ohos --version "$VERSION"
cargo xtask release --component manager --version "$VERSION" --scope all --publish
```

快照和目录命令见[元数据](metadata.md)，原生包格式见[核心 ABI](core-abi.md)。

## 发布历史

发布说明：[beta.4](releases/manager-0.1.0-beta.4.md)、[beta.5](releases/manager-0.1.0-beta.5.md)、[beta.6](releases/manager-0.1.0-beta.6.md)、[beta.8](releases/manager-0.1.0-beta.8.md)。

**历史发行版：Manager 0.1.0-beta.8。** tag [`manager-v0.1.0-beta.8`](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.8) 指向源码提交 [`051a05bceff139e60ef6f973781ffa1f093fb2fc`](https://github.com/Project-Shinkirou/ShinkiEMU/commit/051a05bceff139e60ef6f973781ffa1f093fb2fc)。本版于 2026 年 11 月 3 日 07:04:31 UTC 公开，共有 19 个资产：九个平台包、九份对应源码包和 OHOS 交接记录。[同提交 CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/runs/41543961859) 全矩阵通过，[OHOS 交接 gate](https://github.com/Project-Shinkirou/ShinkiEMU/actions/runs/41576487998) 也已通过。

| 平台 | beta.8 实际交付 |
| --- | --- |
| Windows x86_64、Linux x86_64、macOS aarch64/x86_64 | Manager 桌面包，独立核心另行安装。 |
| Android | 一个通用正式发行 APK，包含 aarch64 和 x86_64 两个 ABI，内嵌 8 核：RFVP、Siglus、Artemis、Musica、KiriKiri2、CMVS（本版新增）、Impacto、Ren'Py。 |
| iOS | 三个测试包：aarch64 device-test、aarch64 simulator-test、x86_64 simulator-test。每包内嵌 6 核：RFVP、Siglus、Musica、Sena、Ethornell、ReMinori。 |
| OHOS aarch64 | 实验性 debug-test 签名 HAP，内嵌 1 核 Musica，使用 OpenHarmony SDK 测试 PKI。设备/模拟器启动和零售设备安装性仍未验证。 |

Android 正式发行证书 SHA256 仍为 `c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc`。发行资产清单和该源码提交的 [`build/cores.toml`](https://github.com/Project-Shinkirou/ShinkiEMU/blob/051a05bceff139e60ef6f973781ffa1f093fb2fc/build/cores.toml) 记录交付目标与内嵌核心；Android 的两个 ABI 共用一个 APK。新 Musica 补丁需要 Musica 核心 `0.1.0-beta.5` 或更高版本。**Manager beta.7 及更早的公开 tag 和资产保持不变。**

**历史发行版：Manager [0.1.0-beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7)。** 本版从 `fd9c78a4cabf1d80430744e4bb5d140c2a837bb9` 公开九个目标。Android 内嵌七核；iOS 的三个设备/模拟器测试包内嵌六核；OHOS 使用 SDK debug-test-PKI 测试签名。官方 Windows 二进制内部自报 `0.1.0-beta.6`。后续源码功能和核心 pin 更新不改变这些历史下载包。

[KiriKiri2 序号 2 发行版](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-kirikiroid2-web-branch-web-1e3603089b4951c8e2831d978ea6fe5a8097bceeb0a56fdeb65aa3953e67eecf) 已从产品 `fe4e6f2d82c8b61f3f14e13d48af6d57722b9952`、适配提交 `d72046c28e6ada623acdb1d72f7075edd488797d` 公开 Windows、Linux 和两种 macOS 架构。相同适配已静态嵌入 beta.7 Android，不代表 KiriKiri 已进入 iOS 或 OHOS。桌面包附对应源码和原许可证，FMOD 条款单独保留。

[Impacto 序号 3 发行版](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-impacto-tag-0-10-8-930-96de7434c8b1a57df7ff24dcee829282194873fe596677056fa168485cb1555e) 已公开 Windows、Linux 和两种 macOS 架构的包，产品提交为 `5b197c1c656186d3a535bb3486311623516166e5`，上游为 `0.10.8.930`，适配提交为 `34b89c29123d025d96f73ac9a746fb388e572065`。四个平台的对应源码、原生输入、许可和包审计均通过，八个公开二进制/源码附件也已无认证实际下载并核对哈希。此独立发行不替换 Manager6 内置的 `910a820ba3ea8d4dd8d6e19c560df1d5a3bc5157`/序号 2，也不改写已公开的序号 1 资产；目录按序号选中 3，不按发布完成先后排序。

[Ren’Py 序号 1 发行版](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-renpy-tag-8-5-3-26051504-5f537560e1a1b852070dec65946d6c027e9f4f7482bddd8349ed47960c383133) 已公开四个桌面平台，产品提交为 `05326439dcfafb81df79f3dabe0997fb696ad3f3`，上游为 `8.5.3.26051504`，适配提交为 `a52d9d9c006223c2b5a276c05e1b97ffeecbe66f`。每份真实对应源码包的固定重构建配方均逐项核验通过 389/389，包括 107 字节的符号链接目标。八个公开二进制/源码附件已无认证实际下载，哈希均与已验收记录一致；原生许可正文也已逐字节核对固定 fork 和原始输入。
