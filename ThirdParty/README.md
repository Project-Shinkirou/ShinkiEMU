# Third-party sources / 第三方源码

外部核心保留完整 fork、上游历史、作者归属和原生实现，由 `build/cores.toml` 的精确提交固定来源。适配代码和构建入口通过普通提交和 PR 维护，不要求压缩成单个适配提交，也不改上游默认分支。主仓不复制第二份外部核心执行器。

External cores retain their complete fork history, upstream attribution and native implementation. Exact source revisions in `build/cores.toml` pin the source. Adapters and build entry points use normal commits and PRs; the upstream default branch is retained.

核心唯一构建清单为根目录 `build/cores.toml`，记录精确 plugin_id、manifest、package、features、许可输入及目标状态。外部 plugin_id 保留上游仓库名和大小写。`kirikiri2/` 对应组织仓库 `kirikiroid2-web`。Musica 与 CMVS 在 `Source/Families/`，不是第三方 fork。

The authoritative build inventory is `build/cores.toml` at the repository root. It supplies exact plugin IDs, manifests, packages, features, license inputs and target status. External IDs keep upstream names and case. The `kirikiri2/` checkout corresponds to `kirikiroid2-web`; Musica and CMVS are in-tree cores under `Source/Families/`.

## ABI and build ownership / ABI 与构建职责

所有核心使用 ShinkiEMU 的 `shinki-emu-abi`（C ABI v3），C++ 使用同一来源生成的 `shinki_emu.h`。共享底层库来自精确 revision 的 RoukakuEngine。VM、原生图形、音频、菜单及存档仍由核心负责；SDK 只复用公共机制。

Each core's `build.rs` validates its own native inputs and writes `core-build.json`. Native CMake targets remain real shared libraries. xtask builds each component through its recorded manifest and collects Cargo-reported outputs. Ren’Py packages need the matching official runtime and `Resources/`, not just a core DLL.

Artemis 保留 Lua 5.1，与 Manager 的 Luau 依赖图分开。CPAL 保留产品的原生音频修复；mlua 0.12.2 已包含精确数字分类，使用 crates.io 版本，删除旧回补。不得把所有 fork 合并成一个 workspace-wide all-features 构建。

Artemis's Lua 5.1 graph remains independent of Manager's Luau graph. CPAL retains the product's native-audio fixes. Registry mlua 0.12.2 includes exact numeric classification and replaces the old vendored backport.

The iOS Manager embeds RFVP, Siglus, Sena, Ethornell and ReMinori from the `shinki` revisions in `build/cores.toml`; the Android Manager embeds those five plus Shinki Musica. Artemis remains a standalone desktop core: its Android embedding is blocked because its Lua 5.1 (`mlua lua51`) conflicts with the Manager's Luau, and one binary can link only one Lua. Manager pins are checked against that inventory. The workspace root also patches Ruffle's `rust-flash-lso` Git source to `Project-Shinkirou/rust-flash-lso@5573168345ef2d92d01ee0766bab29acf89584c0` (`shinki`), preserving upstream `61b717248aae853a4f5d8a103eba704268d286c8` with only its exact thiserror constraint relaxed to compatible 2.x. Cargo.lock records the source, and `cargo vendor` includes it in the corresponding-source closure.

构建命令见[中文文档](../docs/zh-cn/build-and-release.md) / [English guide](../docs/en/build-and-release.md) / [日本語ガイド](../docs/ja/build-and-release.md)，格式与行为见[核心资料](../docs/zh-cn/index.md)。许可和分发义务见[第三方声明](../THIRD_PARTY_NOTICES.md)，历史来源见 [ORIGINS](../ORIGINS.md)。构建成功不代表附件已获分发许可或游戏已验证可玩。
