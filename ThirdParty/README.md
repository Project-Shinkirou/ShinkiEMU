# Third-party sources / 第三方源码

外部核心保留完整 fork 历史、上游署名和原生实现。在根 `Cargo.toml` 固定 Git 提交，由 Cargo 获取；公开源码是 `Cargo.lock` 记录的公开 fork 提交。适配代码和构建入口通过普通提交和 PR 维护，保留上游默认分支。

External cores retain their fork history, upstream attribution and native implementation. Cargo fetches exact Git revisions from the root `Cargo.toml`. The published source is the public fork commit in `Cargo.lock`. Adapters and build entry points use normal commits and PRs and retain the upstream default branch.

`build/cores.toml` 记录 plugin ID、许可输入、目标状态、嵌入范围与上游身份。外部 ID 保留上游仓库名和大小写。构建独立核心时选 `Source/Cores/<id>` 下的 Cargo 入口包。Musica 和 CMVS 的实现在 `Source/Families/`。

`build/cores.toml` supplies plugin IDs, license inputs, target status, embedding and upstream attribution. External IDs keep upstream names and case. Build a standalone core through the Cargo entry package under `Source/Cores/<id>`. Musica and CMVS are implemented in-tree under `Source/Families/`.

[Project-Shinkirou/krkr2](https://github.com/Project-Shinkirou/krkr2) is independent of kirikiroid2-web. Its own `shinki/` adapter and static symbol closure do not link or forward to kirikiroid2-web.

## ABI and build ownership / ABI 与构建职责

所有核心使用 ShinkiEMU 的 `shinki-emu-abi`（C ABI v3），C++ 使用同一来源生成的 `shinki_emu.h`。共享底层库来自精确 revision 的 RoukakuEngine。VM、原生图形、音频、菜单及存档仍由核心负责；SDK 只复用公共机制。

Cargo build scripts drive native CMake builds and supply the resulting libraries and runtime inputs to packaging. Rust standalone cores build through their entry crates. Ren’Py packages need the matching official runtime and `Resources/` alongside the library.

Artemis 在所有平台都使用 Luau，并以独立核心包接入 Android Manager。CPAL 保留产品的原生音频修复；crates.io 上的 mlua 0.12.2 已包含精确数字分类，取代了旧回补。按组件构建，不对整个工作区启用全部 feature。

Artemis uses Luau on every platform and integrates with Android Manager as a standalone core package. CPAL retains the product's native-audio fixes. Registry mlua 0.12.2 includes exact numeric classification and replaces the old vendored backport.

The iOS Manager packages each `ios_framework` core from `build/cores.toml` as an independently signed Framework. Manager loads core binaries dynamically. The root patch for Ruffle's `rust-flash-lso` uses `Project-Shinkirou/rust-flash-lso@5573168345ef2d92d01ee0766bab29acf89584c0` (`shinki`), preserving upstream `61b717248aae853a4f5d8a103eba704268d286c8` with only its exact thiserror constraint relaxed to compatible 2.x.

构建命令见[中文文档](../docs/zh-cn/build-and-release.md) / [English guide](../docs/en/build-and-release.md) / [日本語ガイド](../docs/ja/build-and-release.md)，格式与行为见[核心资料](../docs/zh-cn/index.md)。许可和分发义务见[第三方声明](../THIRD_PARTY_NOTICES.md)，历史来源见 [ORIGINS](../ORIGINS.md)。构建成功不代表附件已获分发许可或游戏已验证可玩。
