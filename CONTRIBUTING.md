# Contributing / 贡献

## 准备环境 / Setup

用 `git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU` 克隆；已有工作副本时运行 `git submodule update --init --recursive` 初始化 `ThirdParty/cpal`。Cargo 根据根目录 `Cargo.toml` 和 `Cargo.lock` 获取外部核心。

Clone with `git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU`, or run `git submodule update --init --recursive` to initialize `ThirdParty/cpal` in an existing clone. Cargo fetches external cores using the root `Cargo.toml` and `Cargo.lock`.

外部贡献者的构建目标是 Windows x64 和 Android ARM64：

- **Windows x64**（`x86_64-pc-windows-msvc`）：安装 Visual Studio 的 x64 C++ 工具、CMake、Ninja、Git 和 `rust-toolchain.toml` 固定的 Rust。在 x64 开发者环境中运行 Cargo 和打包命令（`vcvars64.bat`，PowerShell 用 `Launch-VsDevShell.ps1 -Arch amd64`）。打包从 `%VCToolsRedistDir%\x64` 获取 MSVC 运行库。
- **Android ARM64**（`aarch64-linux-android`）：运行 `rustup target add aarch64-linux-android` 和 `cargo install cargo-ndk --locked --version 4.1.2`，将 `ANDROID_NDK_HOME` 指向 NDK `30.0.16248370`。Manager APK 还需要 JDK 27、SDK platform 37.2 和 build-tools 37.0.0；非 Linux 主机需要对应 ABI 的 `RENPY_STATIC_ROOT`。

Contributor build targets are Windows x64 and Android ARM64:

- **Windows x64** (`x86_64-pc-windows-msvc`): install the Visual Studio x64 C++ tools, CMake, Ninja, Git and the Rust pinned by `rust-toolchain.toml`. Run Cargo and packaging in an x64 developer shell (`vcvars64.bat`, or `Launch-VsDevShell.ps1 -Arch amd64` in PowerShell). Packaging takes MSVC redistributables from `%VCToolsRedistDir%\x64`.
- **Android ARM64** (`aarch64-linux-android`): run `rustup target add aarch64-linux-android` and `cargo install cargo-ndk --locked --version 4.1.2`; point `ANDROID_NDK_HOME` to NDK `30.0.16248370`. The Manager APK also needs JDK 27, SDK platform 37.2 and build-tools 37.0.0. On non-Linux hosts set `RENPY_STATIC_ROOT` to the matching ABI's runtime for Manager.

PR CI 还会构建受影响组件的 Linux x64 行，依赖清单见[构建文档](docs/zh-cn/build-and-release.md#linux-桌面)。macOS、iOS 和 Android Manager（含静态嵌入的核心）由维护者通过完整 `ci.yml` 和发布流程构建，OHOS 由维护者在本机 DevEco 打包。`build/cores.toml` 里已声明的支持目标不能为了通过检查而删除；新增目标只有真实构建成功后才写进 `supported_targets`，否则写带具体原因的 `[[cores.blocked]]`。

PR CI also builds the Linux x64 rows of affected components; its package list is in [Build and release](docs/en/build-and-release.md#linux-desktop). Maintainers build macOS, iOS and the Android Manager (with its statically embedded cores) through the full `ci.yml` and release workflows, and package OHOS locally with DevEco. Never remove an existing supported target to pass a check. List a new target in `supported_targets` only after a real build of it succeeded; otherwise add a `[[cores.blocked]]` entry with a concrete reason.

## 命令 / Commands

从仓库根目录运行。`cargo xtask` 是 Cargo 别名 `run --locked --package xtask`：

Run from the repository root. `cargo xtask` is the Cargo alias `run --locked --package xtask`:

```sh
cargo xtask matrix
cargo build --locked --release -p shinki-emu-manager
cargo build --locked --release -p shinki-emu-core-rfvp
cargo ndk -t arm64-v8a -P 28 build --locked --release -p shinki-emu-core-siglus_rs
cargo check --locked -p shinki-emu-core-rfvp
cargo test --locked -p shinki-emu-musica
cargo xtask package --target <triple> (--manager | --core <plugin_id>) [--version <semver>] --out <new-directory>
cargo xtask metadata validate
cargo xtask release --component <plugin_id> --dry-run
```

首次打包前执行 `cargo install cargo-about --locked --version 0.9.3`。Manager 和自研核心打包时传 `--version`，外部核心省略。`package` 会运行 release 构建，`release --dry-run` 只列出版本、标签和目标。

Before packaging, run `cargo install cargo-about --locked --version 0.9.3`. Pass `--version` for Manager and in-tree core packages; omit it for external cores. `package` runs the release build; `release --dry-run` prints the version, tag and targets.

xtask 调用 Cargo 都带 `--locked`。新增 workspace 成员或依赖后，先不带 `--locked` 执行一次 `cargo metadata --format-version 1 > /dev/null`，再检查 `git diff -- Cargo.lock` 只多出这次改动引入的条目。不要用 `cargo update` 或 `cargo update -p`。细节见[构建文档](docs/zh-cn/build-and-release.md#输入与依赖)。

xtask runs Cargo with `--locked`. After adding a workspace member or dependency, run `cargo metadata --format-version 1 > /dev/null` once without `--locked`, then check that `git diff -- Cargo.lock` adds only what the change introduces. Never use `cargo update` or `cargo update -p`. See [Build and release](docs/en/build-and-release.md#inputs-and-prerequisites).

## 分支与 PR / Branches and PRs

从 `main` 建立功能分支，提交聚焦的普通 PR，说明行为变化、复现方式和实际运行的检查。源码、调用方、有效行为回归和文档一起更新。

Work on a topic branch from `main` and open a normal, focused PR describing behavior, reproduction and checks actually run. Update callers, behavior regressions and docs with the implementation.

**PR 门禁**是构建和打包。改了核心，就按它在 Windows x64 和 Android ARM64 上声明支持的目标运行 Cargo release 构建和 `package`。改了 Manager，就跑 Windows x64 的 Manager 构建和打包。Android 只有 ARM64 环境时，可用 `cargo xtask package --target aarch64-linux-android --manager --version <semver> --qa-single-abi --out <new-directory>` 生成调试签名的单 ABI QA APK。PR CI 再跑 tools 任务和受影响组件的 Windows/Linux 行。`cargo test -p <package>`、ABI 探针、headless 回放和示例运行是可选检查。跑过的话，在 PR 描述里写明命令和结果。截图、日志、hash 和运行输出不提交进仓库。

**The PR gate is build and package.** For a core change, run a release Cargo build and `package` for each of Windows x64 and Android ARM64 that the core declares supported. For a Manager change, build and package the Windows x64 Manager. With only an ARM64 Android environment, `cargo xtask package --target aarch64-linux-android --manager --version <semver> --qa-single-abi --out <new-directory>` produces a local debug-signed single-ABI QA APK. PR CI runs the tools job and the affected Windows/Linux rows. `cargo test -p <package>`, ABI probes, headless replays and the example walkthrough are optional. Summarize checks you ran in the PR. Keep screenshots, logs, hashes and run output out of the repository.

## 核心 / Cores

- 基于 SDK 写新核心：读 [shinkiemu-new-core](.agents/skills/shinkiemu-new-core/SKILL.md)。示例核心 `Source/Examples/shinki-emu-tilefix`（ID `shinki.example.tilefix`）用自造数据演示 SDK API，可直接用 Cargo 构建，不登记到官方目录。
- 移植现有引擎：读 [shinkiemu-integrate-existing-core](.agents/skills/shinkiemu-integrate-existing-core/SKILL.md)。在个人 GitHub fork 上开发 adapter，用本地 `[patch]` 验证。维护者将审查过的提交推到 `Project-Shinkirou/<repo>` 后，ShinkiEMU 的 PR 新增或更新根 `Cargo.toml` 的 `git`/`rev` 和 `Cargo.lock`，并在 `Source/Cores/<id>` 接上入口 crate。原作者仓库的 `upstream_identity` 和 `release_sequence` 放在 `build/cores.toml`。
- 只有维护者能往组织 fork、`main`、tag 和 catalog 写入。贡献者需要有权按上游许可再分发代码和二进制；保留上游 `LICENSE`、署名和 notice，在 fork 里写 `MODIFICATIONS.md`。外部核心的公开源码是 `Cargo.lock` 上记录的公开 fork 提交。GPL/AGPL 核心默认以独立核心包分发；静态嵌入 Manager 之前需要维护者单独评审许可及对应源码义务。

- New SDK core: read [shinkiemu-new-core](.agents/skills/shinkiemu-new-core/SKILL.md). The example core `Source/Examples/shinki-emu-tilefix` (ID `shinki.example.tilefix`) demonstrates the SDK API with generated data. Build it directly with Cargo; it is outside the official catalog.
- Porting an existing engine: read [shinkiemu-integrate-existing-core](.agents/skills/shinkiemu-integrate-existing-core/SKILL.md). Develop the adapter in your personal GitHub fork and verify it with a local `[patch]`. Once a maintainer pushes the reviewed commit to `Project-Shinkirou/<repo>`, the ShinkiEMU PR adds or updates the root `Cargo.toml` `git`/`rev` and `Cargo.lock`, and wires the entry crate under `Source/Cores/<id>`. The original repository's `upstream_identity` and `release_sequence` go in `build/cores.toml`.
- Only maintainers write to organization forks, `main`, tags and the catalog. Contributors must have the right to redistribute code and binaries; keep upstream `LICENSE`, attribution and notices and record changes in the fork's `MODIFICATIONS.md`. Each external core's published source is the public fork commit in `Cargo.lock`. GPL/AGPL cores default to standalone distribution; static Manager embedding requires a separate maintainer review of license and corresponding-source obligations.

## 检查与测试 / Checks and tests

按改动选择 Manager 或单个核心，避免混合独立核心的依赖图。原生测试可能需要真实 GPU、音频设备和公开夹具。用 `cargo xtask matrix` 查看目标，依赖见[构建文档](docs/zh-cn/build-and-release.md)。

Select the component you changed rather than combining independent native dependency graphs. Native regressions may require real GPU/audio devices and public fixtures. Use `cargo xtask matrix` for targets and [Build and release](docs/en/build-and-release.md) for prerequisites.

保留格式边界、损坏/超限输入、存档保护、取消、背压、迟到结果、线程退出、CPU 租约和真实消费者回归。只固定文案、常量、路径拼写、参数转发或 mock 调用的测试不保留。不要删除失败的行为回归或关闭真实后端来通过检查。

Keep behavior regressions for format/budget boundaries, corrupt inputs, save integrity, cancellation/backpressure, stale results, worker shutdown, CPU leases and real consumers. Remove tests that only pin wording, constants, path spelling, forwarding or mock calls. Do not delete failing behavior tests or disable real backends to pass checks.

## 界面翻译 / Manager translations

Manager 以简体中文为源语言：Slint 的 `@tr("…")` 和 Rust 的 `tr!("…")` 都以中文原文作为 msgid，英文和日文译文在 `Source/Manager/shinki-emu-manager-ui-slint/translations/{en,ja}/LC_MESSAGES/shinki-emu-manager-ui-slint.po`。新增或修改界面文字时，在两个 `.po` 文件中同时添加对应条目；缺少条目或译文为空时，构建会以 `SHINKI_EMU_I18N_MISSING` 失败并列出 msgid。一条 msgid 对应一句完整的话，不要拼接片段；变量用 `{}`，有多个变量或语序需要调整时用 `{0}`、`{1}`。术语与 `docs/en`、`docs/ja` 保持一致。逻辑判断比较 id 或枚举，不比较显示文本。

Simplified Chinese is the source language. Slint `@tr("…")` and Rust `tr!("…")` both use the Chinese text as the msgid; English and Japanese live in `Source/Manager/shinki-emu-manager-ui-slint/translations/{en,ja}/LC_MESSAGES/shinki-emu-manager-ui-slint.po`. Add every new or changed string to both files; a missing or empty entry fails the build with `SHINKI_EMU_I18N_MISSING` and lists the msgid. Keep one msgid per complete sentence instead of concatenated fragments; use `{}` for a value, or `{0}`/`{1}` for several values or when word order differs. Follow the terminology in `docs/en` and `docs/ja`. Compare ids or enums in logic, never displayed text.

## 来源、许可与敏感数据 / Sources, licensing and sensitive data

保留作者、上游来源和完整许可；引入依赖或可分发附件前核实分发条件。不得提交商业游戏资源、正文、密钥、私有配置、商业存档、解码导出或含敏感数据的日志。回归使用公开授权或自造夹具。兼容性与补丁通过 issue 表单和维护者审查贡献，规则见 [Metadata](docs/zh-cn/metadata.md)。

Retain author attribution, upstream references and complete licenses. Check distribution rights before adding dependencies or attachments. Never commit commercial assets/text/saves, keys, private profiles, decoded exports or sensitive logs. Use public licensed or self-created fixtures. Compatibility and patch contributions use issue forms and maintainer review; see [Metadata](docs/en/metadata.md).
