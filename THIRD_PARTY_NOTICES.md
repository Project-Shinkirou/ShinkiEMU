# ShinkiEMU Third-Party Notices

本文件随 ShinkiEMU Windows 应用分发。发布包还需包含活动依赖的完整许可证文本；以下条目记录需要单独保留的来源与归因。

## steamroom / steamroom-client 0.3.0

- Upstream: <https://github.com/landaire/steamroom>; published crates pinned to `0.3.0`, by Lander Brandt.
- License: MIT OR Apache-2.0. Manager uses this clean-room Steam protocol and depot downloader implementation, not DepotDownloader binaries or GPL code. Distribution must retain both crates' upstream license texts and the licenses of their resolved dependency closure.

## Slint 1.18.1

- Copyright © SixtyFPS GmbH.
- License mode: Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0.
- License: <https://slint.dev/terms-and-conditions>
- ShinkiEMU 在 设置→关于 中直接显示 Slint 官方 `AboutSlint` widget，该位置可从一级导航“设置”到达。不得从 shipping UI 删除它或把它放到无法从一级导航到达的位置。
- Slint 同时提供其他许可证；RoukakuEngine workspace 不因本产品选择 Royalty-free 2.0 而改为 GPL。

## Material Components for Slint 1.1.0

- Upstream: <https://material.slint.dev/zip/material-1.1.0.zip>（SHA-256 `83c57b6f1b8799ddacd94b7e8b140f3f2112c36407b6df31883da0ba4e2dd425`），原样保存在 `Source/Manager/shinki-emu-manager-ui-slint/material-1.1.0/`，未作修改。
- License: MIT。Copyright (c) 2026 SixtyFPS GmbH。完整文本见 `material-1.1.0/LICENSE.md`，运行包复制为 `licenses/slint-material/LICENSE.md`。

## Material Symbols

- Upstream: <https://github.com/google/material-design-icons>（`49d4db35df873165d6bd6ba09b063c7dafbac2f4`），`symbols/web/<name>/materialsymbolsrounded/<name>_24px.svg`，原样保存在 `Source/Manager/shinki-emu-manager-ui-slint/ui/icons/`。
- License: Apache-2.0。上游 `LICENSE` 为 Apache License 2.0 原文，未填写单独的版权行；完整文本见 `ui/icons/LICENSE`，运行包复制为 `licenses/material-symbols/LICENSE`。

## mlua 0.10.5

- Upstream: <https://github.com/mlua-rs/mlua/tree/v0.10.5>，MIT；完整许可和源码保存在 `ThirdParty/mlua`。
- 本地回补仅修改 `src/state/raw.rs` 的数字分类：要求浮点数与整数精确相等，并保留 IEEE 负零；不以 epsilon 将极小非零值转成 `0`。修复同时用于严格文件范围和 JSON/TOML 数值转换，Luau 运行时版本、API 与执行队列不变。
- Luau 不区分整数和同值的整值浮点字面量；数据编码保留数值，不保证保留原 TOML 数字字面量的类型拼写。
- 本地包在 `licenses/rust/mlua-0.10.5/LICENSE` 包含此 MIT 许可；固定依赖由 ShinkiEMU lockfile 和源码路径复现。

## Luau 663 / luau0-src 0.12.3

- Upstream: <https://github.com/luau-lang/luau>，MIT；ShinkiEMU lockfile 固定 `luau0-src 0.12.3+luau663`。
- `luau0-src` 的官方 `LICENSE` 同时保留 Aleksandr Orlenko、Roblox Corporation 和 Lua.org/PUC-Rio 的 MIT 条款，运行包逐字复制为 `licenses/rust/luau0-src-0.12.3+luau663/LICENSE`。
- 打包器通过目标平台筛选的 `cargo metadata` 找到该精确版本的源码，与 Manager 完整 Rust 依赖闭包一起收集许可文件和 SPDX 声明，不生成替代许可文本。

## aPLib 通用解码

离线补丁库的有界 Rust aPLib 解码按 [apultra 的 expand.c](https://github.com/emmanuel-marty/apultra/blob/master/src/expand.c) token 协议改写，保留 Emmanuel Marty（2019）的 zlib 许可及修改归属，完整文本在 `Source/Tools/shinki-emu-patch/src/codecs.rs` 中。该通用算法不包含商业游戏资源、密钥或文件偏移；运行包须保留许可文本。

```text
Copyright (C) 2019 Emmanuel Marty

This software is provided 'as-is', without any express or implied
warranty. In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```

## ASS 字幕依赖

`roukaku-subtitle` 使用 <https://github.com/libass/libass> 的 libass 0.17.4#1（ISC），原生依赖安装在独立 vcpkg 树中，使用 overlay triplet `x64-windows-static-md-dynbidi`（RoukakuEngine 的 `Engine/Source/Runtime/roukaku-subtitle/triplets/`）：libass、FreeType、HarfBuzz、Brotli、bzip2、libpng、zlib 静态链接，只有 FriBidi 构建为 DLL。RFVP 原生源码的 manifest 与独立 lockfile 固定共享实现 `Project-Shinkirou/RoukakuEngine`；Kira 另行固定。打包时显式指定该实际安装的 `share/`，逐字保留以下 `copyright` 文件到 `licenses/subtitle/<port>/copyright`：

- libass（ISC）、FreeType（本产品选择 FreeType License）、HarfBuzz（Old MIT 及其附带条款）；
- FriBidi（LGPL-2.1）、Brotli（MIT）、bzip2、libpng、zlib。

本产品使用 FreeType Project（<https://freetype.org/>）的软件，保留其完整许可与归因。闭包包含 FreeType 启用的 Brotli/bzip2/PNG/zlib，不能只附 libass 自身的许可。没有复制 DLL 的静态依赖仍须保留条款。

FriBidi 以 `fribidi-0.dll` 动态链接，随使用字幕的 FVP 与 Musica 核心放在各自核心目录（`cores/<stem>/`），用户可替换为自行构建的兼容 DLL。两个核心目录中的同名 DLL 在进程内只加载一份，因此同一 Release 的核心来自同一次 vcpkg 构建。Release 资产附 LGPL-2.1 文本，以及 FriBidi 的对应源码包（vcpkg `downloads/` 中的原始归档与 port 目录，含 portfile 和补丁）。

## ShinkiEMU C ABI v3

外部核心的 Rust 适配与 KrKr 头文件均固定 [`Project-Shinkirou/ShinkiEMU`](https://github.com/Project-Shinkirou/ShinkiEMU) 的 `shinki-emu-abi`（C ABI v3），源码路径为 `Source/FamilyApi/shinki-emu-abi`。外部核心在各自 fork 的 manifest 上独立构建；ShinkiEMU 工作区只在 Android Manager 静态链接 `shinki-emu-rfvp` 时经 `[patch]` 把同一来源映射到本地源码。

外部核心的适配代码全部位于 fork `shinki` 分支上的单个提交，主仓不保留适配源码。plugin ID 等于上游仓库名，适配包命名为 `shinki-emu-<上游仓库名>`。

## ReMinori

- 上游：<https://github.com/luoyily/ReMinori>，基线 `79b00990324404bfbd496e7fa76c690cf040f744`；作者 luoyily 及上游贡献者，许可为 GNU Affero General Public License version 3。
- 适配 fork：<https://github.com/Project-Shinkirou/ReMinori/tree/shinki>，固定 `5ce0c4f55b5f69bbbca0478ced121bf31a0d0851`；适配包为 `crates/shinki`，plugin ID 保留 `ReMinori` 大小写，family ID 为 `musica`，不替代自有 Musica 核心或复用其执行器。
- 适配保留原 VM、GPU renderer、CPAL mixer、Media Foundation 视频、游戏 UI 和存档；新增生产 C ABI v3、固定三槽 CPU lease、独立保存位置、keys.v2 归档参数、严格 CP932/GBK 脚本读取、原生 UTF-8 port 保存及多声道设备接线。修改归属与代表流程见 fork 根 `README.md`。
- 本地核心包附原样 `LICENSE` 和该 `README.md`。AGPL 对应源码及组合分发义务须在公开二进制发布前核验；本次仅交付本地验证产物，不把加入主仓等同于更改 RoukakuEngine 或其他核心的许可。商业资源、私有 profile 与测试存档不进入源码或包。

## impacto

- 上游：<https://github.com/CommitteeOfZero/impacto>，revision `914fae7cbb9583f8aa231569a03aab27d8c042f8`；整体 ISC，各原组件沿用其独立许可。
- 适配 fork：<https://github.com/Project-Shinkirou/impacto/tree/shinki>，固定 `991ece88c27736ffccb516ba1ff132fc1fcafc73`。通用原生基线 `f12b4b799f1024eaca9a4e48e0371c2f3835576e` 独立修复 DDS 并行读取与电影音频释放；其上单个 Shinki 适配提交保留原 SC3、OpenGL、OpenAL、FFmpeg、游戏 UI 和原生资源读取。
- 薄嵌入层在 `src/shinki`，固定 C ABI v3 头在 `vendor/shinki-emu-abi/include/shinki_emu.h`，来源为 ShinkiEMU 的 `Source/FamilyApi/shinki-emu-abi`，原样保留并附 MIT OR Apache-2.0 许可。DDS 文件继续保留 OpenImageIO 的 BSD-3-Clause 归属，Steam 字库仅在运行时读取游戏自己的两个原生 DDS。
- 本地核心目录附 `LICENSE`、`THIRDPARTY.md`、`README.md` 和实际 vcpkg 依赖的完整 copyright；FFmpeg 7.1.2、OpenAL Soft、FriBidi 等动态库保留原分发义务，公开二进制发布仍需核验对应源码及许可闭包。
- 本轮保持开源依赖，不复制、加载或分发 Bink DLL。CHAOS;CHILD 验收已按用户要求跳过，既有系统数据错误页诊断不能算可玩通过；商业素材、私有映射与试验存档不进入源码或包。

## Ren'Py

- 上游：<https://github.com/renpy/renpy>，稳定版 `8.5.3.26051504`，基线 `39895c1e017f0b36ffea2447d97eccd69d76ee1c`；适配 fork 为 <https://github.com/Project-Shinkirou/renpy/tree/shinki>，固定 `fea2918fa2a5f47c4cda1e7a5f02b84f45f23e00`，薄接入在 `shinki/`。
- 执行、Screen Language、ATL、GL2、音视频及保存沿用官方实现。CPython 3.12.7、原生扩展和同一份 SDL 2.0.20 取自官方 SDK；仅重新编译必要的 display 模块，不复制原游戏可执行文件或 DLL。来源及五个 SDK DLL 的固定校验和见 fork 的 `shinki/sdk.json`。
- 适配与共享 C ABI 的 MIT 许可见 fork `shinki/LICENSE`，原文件保留原许可。核心包附官方完整 `LICENSE.txt`、pthread 说明与修改说明；Ren'Py 及依赖中的 MIT、Zlib、BSD、Python 与 LGPL 等许可各自保留，不能将整个运行时标成纯 MIT。公开分发仍需满足 LGPL 对应源码及重新链接义务。
- 包内不含 Steam、Cubism、Bink、nvdrs 或微软 D3DCompiler 再发行 DLL；ANGLE 如需 D3DCompiler，使用 Windows 系统组件。商业资源、存档及验证画面不进入源码或核心包。

## 离线补丁数据原语

- Pickle 数据编解码复用 `serde-pickle 1.2.0`（MIT OR Apache-2.0），借用式安全前置使用 `repugnant-pickle 0.0.1`（Apache-2.0）及 `nom 7.1.3`（MIT）；来源分别为 <https://crates.io/crates/serde-pickle>、<https://crates.io/crates/repugnant-pickle>、<https://crates.io/crates/nom>。`repugnant-pickle` 关闭默认 feature，不引入其可选 ZIP/Torch 处理。
- 这些库只处理有界数据，不执行 Python 对象；RPA 索引列交换位于 MIT OR Apache-2.0 的 Luau recipe，原生 Ren'Py reader 保持官方格式。归档指纹只用于确定支持的输入版本，不包含商业 payload 或私有 key。

## rfvp derivative used by shinki-emu-rfvp

- Upstream: <https://github.com/xmoezzz/rfvp>
- Upstream version: `0.6.0`, revision `af557db750c7833274f22a6d82b0dc33cdea2f85`
- Native adaptation fork: <https://github.com/Project-Shinkirou/rfvp>
- Native adaptation branch: `shinki`
- 当前 `shinki` 与主仓 gitlink：[`95d3ee23548cb32e035aaaaeab2d309fc5d3020e`](https://github.com/Project-Shinkirou/rfvp/commit/95d3ee23548cb32e035aaaaeab2d309fc5d3020e)，即通用原生修复基线 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368` 上的单个适配提交（`crates/shinki-emu-rfvp`，C ABI v3 移植并删除 D3D12 交付，加 ABI pin）。此前的 `1e947a5ccbd2c7a698bd30d48347454ef1b908a9`、`ea8b5964fd5b6cde8186eff6a5da1607a3eb6a41` 与 `478448ed9d60778f78d5de911fa5717c48cb201c` 仅作历史记录，不代表当前构建源码。
- License: Mozilla Public License 2.0.
- Derivative source: <https://github.com/Project-Shinkirou/rfvp>
- Shinki adapter source: fork 内 `crates/shinki-emu-rfvp/`
- 统一 C ABI 直接使用原生 App、VM、renderer、设备音频与保存。CPU lease 交付与类型化渲染尺寸（C ABI v3 `render_extent`）位于核心薄嵌入层；主仓不实现 Hosted Tick、Host PCM 或另一套游戏执行。当前原生 typed 配置支持按原 HCB 偏移索引的 UTF-8 翻译表及 movie/BGM ASS 映射，字幕使用显式字体和原媒体时钟绘入场景，沿暂停、结束、seek/读档与尺寸变化更新。

发布时提供与二进制对应的 MPL-2.0 covered source、修改说明和完整许可证，可随包分发源码或提供有效的 source offer。

完整 fork 以 Git submodule 固定在 `ThirdParty/rfvp/`。文件级变化与覆盖范围见 fork 的 `crates/shinki-emu-rfvp/MODIFICATIONS.md` 与 `THIRD_PARTY_NOTICES.md`，二者随核心目录发布。当前原生字体构造嵌入上游四个 MS 命名字体文件，由上游 [`xmoezzz/rfvp@f60d8660`](https://github.com/xmoezzz/rfvp/commit/f60d8660d800fe16a4da485ef50e010e7b1ec2e5) 加入，存在于上游基线 `af557db750c7833274f22a6d82b0dc33cdea2f85`；二进制按上游原样随核心发布。

## KrKr native development bundle

- Upstream: <https://github.com/fenghengzhi/kirikiroid2-web>
- Fixed upstream revision: `7105b50822097d11b7b1f5894bf541509fd51350`。
- 固定 fork revision：[`e681590a1075cb4be4e9d41d99cffc2cdba01bc3`](https://github.com/Project-Shinkirou/kirikiroid2-web/commit/e681590a1075cb4be4e9d41d99cffc2cdba01bc3)，发布分支 `shinki`；它是上游基线 `7105b508` 上的单个适配提交，包含 `shinki/` 适配（C++ 在 `shinki/native`）、`KRKR2_SHINKI_EMU` CMake 选项与 WindowEx overlay owner 初始化修复。此前的 `2532a699b260488db32b32739f9648858c00a9a4` 仅作历史记录。
- 完整 fork 保留独立的 Windows 构建、可重入生命周期、游戏路径缓存与 WindowEx overlay owner 初始化修复。C++ 嵌入适配直接导出统一 ABI，删除 child/IPC，沿用原生 XP3/PSB/KBAD 与字体路径。Release 的实际游戏流程仍由用户人工验收。
- 运行包保留完整核心的实际 `LICENSE`、fork 的 `shinki/README.md` 适配说明、layerExImage 与 DroidSansFallback 许可及构建依赖的 copyright 文件；没有独立许可文本的文件不补造许可证。根许可包含多个组件的实际条款，不用上游 README 的 MIT 概述替代。
- FFmpeg 3.3.9（LGPL-2.1）改为动态链接：适配提交让 libbpg 经公开的 `avcodec_find_decoder_by_name("hevc")` 取得 HEVC 解码器，取代私有符号 `ff_hevc_decoder`，并删除 Windows port 的静态限制。`avcodec-57.dll` 等 FFmpeg DLL 随 KrKr 核心放在其核心目录，可由用户替换。Release 资产附 LGPL-2.1 文本与 FFmpeg 3.3.9 对应源码包（原始归档与 fork 内 `vcpkg/ports/ffmpeg`）。
- 包根的 VC runtime 与 KrKr 核心目录内的 OpenMP runtime（`vcomp140.dll`）取自 MSVC redist。

## Noto 字体

Musica 编译期嵌入 Noto Sans JP 与 Noto Sans SC，使用 SIL Open Font License 1.1。字体通过仓库 Git LFS 提供，运行包在 `licenses/noto/OFL.txt` 保留相应许可；它们不替代 FVP 请求的原生系统字体。

## Musica key.toml 解析

`toml 1.1.3+spec-1.1.0` 来源为 <https://github.com/toml-rs/toml>，许可为 MIT OR Apache-2.0；`winnow 1.0.4` 来源为 <https://github.com/winnow-rs/winnow>，许可为 MIT。具体解析、序列化与关联 crate 版本固定于 ShinkiEMU lockfile；本地运行包在 `licenses/rust/` 保留本轮新增解析依赖的完整许可文本。

## 异步 GIF 封面

封面缩略图使用 `image` 的 GIF 解码支持：[`gif 0.14.2`](https://crates.io/crates/gif/0.14.2) 与 [`weezl 0.1.12`](https://crates.io/crates/weezl/0.1.12) 为 MIT OR Apache-2.0，关联的 [`color_quant 1.1.0`](https://crates.io/crates/color_quant/1.1.0) 为 MIT。本地运行包在 `licenses/rust/` 保留这些版本的完整许可文本。

## WMV decoder

FVP 使用完整 RFVP fork 中的原生 `crates/na_wmv_player` 及原 Movie 路径处理 ASF/WMV/WMA，不再直接依赖主仓 `Source/Families/na_wmv_player`。原生 decoder 清单没有独立 license 字段，须保留完整 fork 许可与来源，并在分发前核实实际覆盖范围，不能从 FFmpeg 算法名称推断额外授权。主仓旧独立 decoder/工具的来源和许可不明问题仍保留在其 `THIRD_PARTY_NOTICES.md`，不能混称为当前 FVP 的直接依赖。打包复制 fork 内 `crates/shinki-emu-rfvp/` 的说明与完整 RFVP 许可，不再附加旧 decoder 的错误归因。

## Anime4K

- Upstream: <https://github.com/bloc97/Anime4K>
- Fixed revision: `7684e9586f8dcc738af08a1cdceb024cc184f426`
- Copyright © 2019–2021 bloc97.
- License: MIT.
- 内置 Restore CNN S 与 Upscale CNN x2 S 从该版本的 GLSL shader 独立移植，随 shader 保留完整 MIT 许可证。Magpie format 4 的兼容解析与内建函数独立实现，不包含 Magpie GPL 实现代码。
- 已按固定版本的 [Restore](https://github.com/bloc97/Anime4K/blob/7684e9586f8dcc738af08a1cdceb024cc184f426/glsl/Restore/Anime4K_Restore_CNN_S.glsl) / [Upscale](https://github.com/bloc97/Anime4K/blob/7684e9586f8dcc738af08a1cdceb024cc184f426/glsl/Upscale/Anime4K_Upscale_CNN_x2_S.glsl) 修正移植中的 8×8 调度覆盖、正负激活、Restore 最后一层输入和放大残差采样坐标；权重不变，完整游戏帧保持不透明 alpha。硬件像素回归与独立 CPU 参考比较链式和单独两种效果，覆盖非整组尺寸与图像边缘。

## DirectX Shader Compiler

- Upstream: <https://github.com/microsoft/DirectXShaderCompiler>
- Fixed version: `1.8.2502`.
- 应用的 HLSL 编译路径使用固定版本 DXC；分发编译器时一并保留官方发行包中的许可证和第三方说明。

## 原生音频依赖

- CPAL 0.18.2：<https://github.com/RustAudio/cpal/releases/tag/v0.18.2>，Apache-2.0，Musica 的原生设备 owner 使用该版本；Manager 不再拥有核心 PCM 输出。
- CPAL 0.15.3：`Project-Shinkirou/cpal@ecfda841a9140d60bbec1630d47ca91f1a77b8f3`，保留 WASAPI 关闭修复并将 ALSA 依赖对齐到 0.11。完整源码与 Apache-2.0 许可位于 `ThirdParty/cpal`。
- Kira 保留各核心原版本，独立 fork 修复 decoder/device worker 的显式关闭。Musica/Artemis 使用 0.12.2 `7d8961b20bce2ab8fd43b4bb34150d7e2898785c`；RFVP 使用 0.10.8 `d2f10db15d9c451c7b2fd33043ae7732e75bee5d`；Siglus 嵌套 0.9.6 `a299b42690094a71885f6ed97dc504c4038da9a7`；Sena 使用 0.12.1 `523790dbb7ddea5ca432f375a9f3180923f2ea1e`；Ethornell 使用 0.8.7 `da8668fcd5e543eec2e9ea9871b310276e09fdc5`。来源均为 <https://github.com/Project-Shinkirou/kira>，保留 MIT/Apache-2.0 文本和修改说明。

## 其他原生核心

- Siglus：当前 `shinki` 与主仓 gitlink 为 [`Project-Shinkirou/siglus_rs@04426d0384dd4e048b41fdd5b0508f0b92a77958`](https://github.com/Project-Shinkirou/siglus_rs/commit/04426d0384dd4e048b41fdd5b0508f0b92a77958)，即通用基线 `13b55b47484faf8dfe03deb6ffd56ca0ec4ba3f2` 上的单个适配提交（`shinki/shinki-emu-siglus_rs`，独立 workspace 与 lockfile）；`85c3a389577e61e3ccccde3d1c84de8bc19b7678`、`a94565cccd4f6d5e0a341e5732c5d98e9de2c71f` 仅作历史记录。MPL-2.0，完整源码及嵌套 Kira、eluna_rs 位于 `ThirdParty/siglus_rs`。当前原生配置支持 original/chs 成对资源、G01/movie 选择及独立保存根，不执行 wrapper 机器码补丁或补造缺失字体。来源与输入边界改动见 fork 的 `shinki/MODIFICATIONS.md` 与 `shinki/THIRD_PARTY_NOTICES.md`。
- Artemis：当前公开 `shinki` 与主仓 gitlink 为 [`Project-Shinkirou/art3m1s-core@5c83084d55573855713feefa949be24f0ae6baee`](https://github.com/Project-Shinkirou/art3m1s-core/commit/5c83084d55573855713feefa949be24f0ae6baee)，即基线 `ad90a62cb192b119b7cf8589aa7f87a94bd747c8` 上的单个适配提交（`crates/shinki-emu-art3m1s-core`，fork 根 workspace 成员）；`be82d5e0bbafcef55a530f69e5b9358094dbd671`、`ae967d8446fe99b97f6e1df93fb1b61d8ada2040` 仅作历史记录。MPL-2.0，保留原生 Lua 5.1/Vulkan/FFmpeg 与完整来源，见 `ThirdParty/art3m1s-core/LICENSE` 与 `crates/pf8/LICENSE`；差异见 fork 的 `crates/shinki-emu-art3m1s-core/MODIFICATIONS.md`。当前 `extra_archive` 直接消费显式额外 PFS，保留基包及原覆盖顺序。
- Sena：SoftPAL 引擎的模拟器，上游 <https://github.com/xmoezzz/sena-rs>；当前 `shinki` 与主仓 gitlink 为 [`Project-Shinkirou/sena-rs@6693a0d84d6adb0672817c8486111c165279ea46`](https://github.com/Project-Shinkirou/sena-rs/commit/6693a0d84d6adb0672817c8486111c165279ea46)，即通用基线 `5fd1a8e37600fb917e053f633671560df9ca42a7` 上的单个适配提交（`crates/shinki-emu-sena-rs`）；`e4e708be94ddecf99741a1177bede5ed296d21f5`、`8465e2818842b95aeb37424104e3ab95feff777d` 仅作历史记录。基线包含新会话读档后重新启用保存的修复；当前嵌入配置提供 `text_language=jp/cn/tc`，原生选择多语言记录字段并保留原日文单字段行为，字体由原生游戏字体路径或独立原生配置的显式字体文件提供；光栅尺寸经 C ABI v3 `render_extent` 请求。完整 MPL-2.0 许可位于 `ThirdParty/sena-rs/LICENSE-MPL-2.0`。支持范围仍为原生及 Steam Koikake，未扩大到其他 SoftPAL。
- Ethornell：上游 <https://github.com/xmoezzz/ethornell-rs>，workspace manifest 声明 `license = "MPL-2.0"`，上游仓库本身未附许可文本，fork 的适配提交在根目录加入 MPL-2.0 全文 `LICENSE-MPL-2.0`；Release 的 Ethornell 核心目录附该全文（`licenses/ethornell-rs/LICENSE-MPL-2.0`）与 fork 的 `MODIFICATIONS.md`。适配源码发布于 [Project-Shinkirou/ethornell-rs 的 shinki](https://github.com/Project-Shinkirou/ethornell-rs/tree/shinki)，当前 `shinki` 与主仓 gitlink 为 [`e1fd89dbc4d6a2f3b15aa882deea2ade73020e42`](https://github.com/Project-Shinkirou/ethornell-rs/commit/e1fd89dbc4d6a2f3b15aa882deea2ade73020e42)，即上游 `6550460beb2012781cdaffdf18da59780de422ef` 上的单个适配提交（`crates/shinki-emu-ethornell-rs`）；`44801f7901199af208410131db757204981519ce`、`e8adc0ca57d163cd9718c157d3f9c106aaf4fde0` 与旧 Release 运行代码 `b6991511b57d8031be14f366732c5f1974937c42` 仅作历史记录。当前原生能力包括仅作用于显示文本的 GBK 解码选择和每 runtime 显式字体，不代表未知补丁 handler、字体注册或资源优先级已经闭合。二进制随 Release 发布，源码以 `ThirdParty/ethornell-rs` submodule 固定。
