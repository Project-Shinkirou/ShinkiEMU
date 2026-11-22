# Contributing / 贡献

## 准备环境 / Setup

用 `git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU` 克隆；已有 checkout 执行 `git submodule update --init --recursive`。工作区通过 `ThirdParty/cpal` 子模块 patch `cpal`，缺少它时所有 Cargo 命令都会报 `patch location ThirdParty\cpal does not contain packages matching cpal`。`ThirdParty/cpal` 是唯一的子模块；外部核心由 `cargo xtask deps` 按 `build/cores.toml` 的 pin 检出到 `ThirdParty/`。

Clone with `git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU`, or run `git submodule update --init --recursive` in an existing checkout. The workspace patches `cpal` from the `ThirdParty/cpal` submodule; without it every Cargo command fails with `patch location ThirdParty\cpal does not contain packages matching cpal`. That is the only submodule; `cargo xtask deps` checks out external cores under `ThirdParty/` at the pins in `build/cores.toml`.

外部贡献者的构建目标是 Windows x64 和 Android ARM64：

- **Windows x64**（`x86_64-pc-windows-msvc`）：Visual Studio 的 x64 C++ 生成工具、CMake、Ninja、Git，以及 `rust-toolchain.toml` 固定的 Rust。`build` 和 `package` 在 x64 开发者环境里执行（`vcvars64.bat`，PowerShell 用 `Launch-VsDevShell.ps1 -Arch amd64`），这个 shell 里要同时能找到 `cl.exe`、CMake、Ninja，并设有 `VCToolsRedistDir`。打包只从 `%VCToolsRedistDir%\x64` 取 `VCRUNTIME140.dll`；`doctor` 在普通 shell 里也会找到 `cl.exe`，所以它通过不代表能打包。
- **Android ARM64**（`aarch64-linux-android`）：`rustup target add aarch64-linux-android`；NDK 必须正好是 30.0.16248370，用 `ANDROID_NDK_HOME` 指向它，或设 `ANDROID_HOME` 让 xtask 使用 SDK 下的 `ndk/30.0.16248370`；`PATH` 上的 `java` 要是 JDK 27，`doctor` 对单独的核心包也检查它。cargo-ndk 4.1.2 由 `cargo xtask deps` 私有安装。构建核心包不用 Gradle 和签名；构建 Manager APK 另需 SDK platform 37.2 和 build-tools 37.0.0。

Contributor build targets are Windows x64 and Android ARM64:

- **Windows x64** (`x86_64-pc-windows-msvc`): Visual Studio x64 C++ build tools, CMake, Ninja, Git and the Rust pinned by `rust-toolchain.toml`. Run `build` and `package` inside an x64 developer shell (`vcvars64.bat`, or `Launch-VsDevShell.ps1 -Arch amd64` in PowerShell) that exposes `cl.exe`, CMake and Ninja and sets `VCToolsRedistDir`. Packaging takes `VCRUNTIME140.dll` only from `%VCToolsRedistDir%\x64`; `doctor` finds `cl.exe` from an ordinary shell too, so a passing `doctor` does not mean packaging will work.
- **Android ARM64** (`aarch64-linux-android`): `rustup target add aarch64-linux-android`; exactly NDK 30.0.16248370, either through `ANDROID_NDK_HOME` or through `ANDROID_HOME`, from which xtask uses `ndk/30.0.16248370`; and JDK 27 as the `java` on `PATH`, which `doctor` checks even for a standalone core package. `cargo xtask deps` installs cargo-ndk 4.1.2 privately. Core packages need no Gradle or signing; the Manager APK additionally needs SDK platform 37.2 and build-tools 37.0.0.

PR CI 还会构建受影响组件的 Linux x64 行，依赖清单见[构建文档](docs/zh-cn/build-and-release.md#linux-桌面)。macOS、iOS 和 Android Manager（含静态嵌入的核心）由维护者通过完整 `ci.yml` 和发布流程构建，OHOS 由维护者在本机 DevEco 打包。`build/cores.toml` 里已声明的支持目标不能为了通过检查而删除；新增目标只有真实构建成功后才写进 `supported_targets`，否则写带具体原因的 `[[cores.blocked]]`。

PR CI also builds the Linux x64 rows of affected components; its package list is in [Build and release](docs/en/build-and-release.md#linux-desktop). Maintainers build macOS, iOS and the Android Manager (with its statically embedded cores) through the full `ci.yml` and release workflows, and package OHOS locally with DevEco. Never remove an existing supported target to pass a check. List a new target in `supported_targets` only after a real build of it succeeded; otherwise add a `[[cores.blocked]]` entry with a concrete reason.

## 命令 / Commands

从仓库根目录运行。`cargo xtask` 是 Cargo 别名 `run --locked --package xtask`：

Run from the repository root. `cargo xtask` is the Cargo alias `run --locked --package xtask`:

```sh
cargo xtask matrix
cargo xtask doctor  --target <triple> [--core <plugin_id>]
cargo xtask deps    --target <triple> [--core <plugin_id>] [--allow-dirty-sources]
cargo xtask build   --target <triple> (--manager | --core <plugin_id>) --release
cargo xtask check   --target <triple> (--manager | --core <plugin_id>)
cargo xtask package --target <triple> (--manager | --core <plugin_id>) [--version <semver>] --out <new-directory>
cargo xtask test    [--manager | --core <plugin_id>]
cargo xtask metadata validate
cargo xtask release --component <plugin_id> --dry-run
```

`doctor` 只诊断；`deps` 是唯一的下载入口；Manager 和自研核心打包要传 `--version`，外部核心不传。`test` 在宿主上运行，不接受 `--target`，不选组件时测试 xtask 和构建辅助库。`release --dry-run` 只验证外部核心的上游和 adapter 身份，不构建、不发布。

`doctor` only diagnoses; `deps` is the only download entry. Manager and self-owned cores pass `--version` to `package`; external cores omit it. `test` runs on the host without `--target`; with no selection it tests xtask and the build helper. `release --dry-run` verifies an external core's upstream and adapter identity without building or publishing.

xtask 调用 Cargo 都带 `--locked`。新增 workspace 成员或依赖后，先不带 `--locked` 执行一次 `cargo metadata --format-version 1 > /dev/null`，再检查 `git diff -- Cargo.lock` 只多出这次改动引入的条目。不要用 `cargo update` 或 `cargo update -p`。细节见[构建文档](docs/zh-cn/build-and-release.md#输入与依赖)。

xtask runs Cargo with `--locked`. After adding a workspace member or dependency, run `cargo metadata --format-version 1 > /dev/null` once without `--locked`, then check that `git diff -- Cargo.lock` adds only what the change introduces. Never use `cargo update` or `cargo update -p`. See [Build and release](docs/en/build-and-release.md#inputs-and-prerequisites).

## 分支与 PR / Branches and PRs

从 `main` 建立功能分支，提交聚焦的普通 PR，说明行为变化、复现方式和实际运行的检查。源码、调用方、有效行为回归和文档一起更新。

Work on a topic branch from `main` and open a normal, focused PR describing behavior, reproduction and checks actually run. Update callers, behavior regressions and docs with the implementation.

**PR 门禁**是构建和打包。改了核心，就对它在 Windows x64 和 Android ARM64 上声明支持的目标各跑一次 `deps`、`build --release`、`package`；改了 Manager，就跑 Windows x64 的 Manager `build` 和 `package`，Android 只有 ARM64 环境时可以用 `package --target aarch64-linux-android --manager --version <semver> --qa-single-abi --out <new-directory>` 生成本地 debug 签名的单 ABI QA APK。PR CI 再跑 tools 任务和受影响组件的 Windows/Linux 行。`test --core`、ABI 探针、headless 回放和示例运行都是可选检查；跑过的话，在 PR 描述里用一两行写结果（命令、退出码、帧数或 hash）。截图、日志、hash、回执和运行输出不提交进仓库。

**The PR gate is build and package.** For a core change, run `deps`, `build --release` and `package` for each of Windows x64 and Android ARM64 that the core declares supported. For a Manager change, run the Windows x64 Manager `build` and `package`; with only an ARM64 Android environment, `package --target aarch64-linux-android --manager --version <semver> --qa-single-abi --out <new-directory>` produces a local debug-signed single-ABI QA APK. PR CI then runs the tools job and the Windows/Linux rows of affected components. `test --core`, ABI probes, headless replays and the example walkthrough are optional; if you ran them, summarize each in a line or two of the PR description (command, exit code, frame count or hash). Do not commit screenshots, logs, hashes, receipts or run output.

## 核心 / Cores

- 基于 SDK 写新核心：读 [shinkiemu-new-core](.agents/skills/shinkiemu-new-core/SKILL.md)。示例核心 `Source/Examples/shinki-emu-tilefix`（ID `shinki.example.tilefix`）用生成的自造小数据演示 SDK API，不属于官方目录；构建它需要在临时 checkout 里追加注册，步骤见该 skill。
- 移植现有引擎：读 [shinkiemu-integrate-existing-core](.agents/skills/shinkiemu-integrate-existing-core/SKILL.md)。adapter 先在你自己的 GitHub fork 上开发并本地验证；维护者再创建或更新 `Project-Shinkirou/<repo>` 组织 fork，把审查过的提交普通 push 进去。ShinkiEMU 的 PR 随后在 `build/cores.toml` 的 `[cores.source]` 写组织 fork 的完整提交、原作者 GitHub 仓库的 `upstream_identity`（tag 或真实分支加完整提交）和 `release_sequence`，用 `cargo xtask release --component <id> --dry-run` 验证。上游必须托管在 GitHub。
- 只有维护者能往组织 fork、`main`、tag 和 catalog 写入。贡献者需要有权按上游许可再分发代码和二进制；保留上游 `LICENSE`、署名和 notice，在 fork 里写 `MODIFICATIONS.md`。GPL/AGPL 核心默认以带对应源码的动态核心包分发；静态嵌入 Manager 之前需要维护者单独做许可和对应源码评审。ReMinori（AGPL）的静态版本在评审完成前不公开发布。

- New SDK core: read [shinkiemu-new-core](.agents/skills/shinkiemu-new-core/SKILL.md). The example core `Source/Examples/shinki-emu-tilefix` (ID `shinki.example.tilefix`) demonstrates the SDK API with tiny generated self-authored data and is not part of the official catalog; building it needs a registration appended in a disposable checkout, as the skill describes.
- Porting an existing engine: read [shinkiemu-integrate-existing-core](.agents/skills/shinkiemu-integrate-existing-core/SKILL.md). Develop the adapter in your own GitHub fork and verify it locally. A maintainer then creates or updates the `Project-Shinkirou/<repo>` organization fork and pushes the reviewed commits there without force. The ShinkiEMU PR pins that full organization commit in `[cores.source]` of `build/cores.toml`, together with `upstream_identity` (original author's GitHub repository, tag or real branch, full commit) and `release_sequence`, and is checked with `cargo xtask release --component <id> --dry-run`. The upstream must be hosted on GitHub.
- Only maintainers write to organization forks, `main`, tags and the catalog. Contributors must have the right to redistribute the code and binaries under the upstream license; keep upstream `LICENSE`, attribution and notices, and record changes in the fork's `MODIFICATIONS.md`. GPL/AGPL cores ship by default as dynamic core packages with corresponding source; static embedding in the Manager needs a separate maintainer license and corresponding-source review first. ReMinori (AGPL) has no public static release until that review is complete.

## 检查与测试 / Checks and tests

按修改选择 Manager 或单个核心，避免混合独立核心的依赖图。原生测试可能需要真实 GPU/音频和公开夹具。目标与依赖以 `cargo xtask matrix` 和 `doctor` 为准，完整入口见[构建文档](docs/zh-cn/build-and-release.md)。

Select the component you changed rather than combining independent native dependency graphs. Native regressions may require real GPU/audio and public fixtures. Use `matrix` and `doctor` for target/prerequisite information; see [Build and release](docs/en/build-and-release.md).

保留格式边界、损坏/超限输入、存档保护、取消、背压、迟到结果、线程退出、CPU 租约和真实消费者回归。只固定文案、常量、路径拼写、参数转发或 mock 调用的测试不保留。不要删除失败的行为回归或关闭真实后端来通过检查。

Keep behavior regressions for format/budget boundaries, corrupt inputs, save integrity, cancellation/backpressure, stale results, worker shutdown, CPU leases and real consumers. Remove tests that only pin wording, constants, path spelling, forwarding or mock calls. Do not delete failing behavior tests or disable real backends to pass checks.

## 界面翻译 / Manager translations

Manager 以简体中文为源语言：Slint 的 `@tr("…")` 和 Rust 的 `tr!("…")` 都以中文原文作为 msgid，英文和日文译文在 `Source/Manager/shinki-emu-manager-ui-slint/translations/{en,ja}/LC_MESSAGES/shinki-emu-manager-ui-slint.po`。新增或修改界面文字时，在两个 `.po` 文件中同时添加对应条目；缺少条目或译文为空时，构建会以 `SHINKI_EMU_I18N_MISSING` 失败并列出 msgid。一条 msgid 对应一句完整的话，不要拼接片段；变量用 `{}`，有多个变量或语序需要调整时用 `{0}`、`{1}`。术语与 `docs/en`、`docs/ja` 保持一致。逻辑判断比较 id 或枚举，不比较显示文本。

Simplified Chinese is the source language. Slint `@tr("…")` and Rust `tr!("…")` both use the Chinese text as the msgid; English and Japanese live in `Source/Manager/shinki-emu-manager-ui-slint/translations/{en,ja}/LC_MESSAGES/shinki-emu-manager-ui-slint.po`. Add every new or changed string to both files; a missing or empty entry fails the build with `SHINKI_EMU_I18N_MISSING` and lists the msgid. Keep one msgid per complete sentence instead of concatenated fragments; use `{}` for a value, or `{0}`/`{1}` for several values or when word order differs. Follow the terminology in `docs/en` and `docs/ja`. Compare ids or enums in logic, never displayed text.

## 来源、许可与敏感数据 / Sources, licensing and sensitive data

保留作者、上游来源和完整许可；引入依赖或可分发附件前核实分发条件。不得提交商业游戏资源、正文、密钥、私有配置、商业存档、解码导出或含敏感数据的日志。回归使用公开授权或自造夹具。兼容性与补丁通过 issue 表单和维护者审查贡献，规则见 [Metadata](docs/zh-cn/metadata.md)。

Retain author attribution, upstream provenance and complete licenses. Check distribution rights before adding dependencies or attachments. Never commit commercial assets/text/saves, keys, private profiles, decoded exports or sensitive logs. Use public licensed or self-created fixtures. Compatibility and patch contributions use issue forms and maintainer review; see [Metadata](docs/en/metadata.md).
