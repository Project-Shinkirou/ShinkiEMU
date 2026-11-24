---
name: shinkiemu-integrate-existing-core
description: Use when porting an existing third-party engine implementation (a Rust crate or a C/C++ project) into ShinkiEMU as an external core, or when changing an existing fork adapter, its probe/open gates, its Cargo pin, upstream_identity or release_sequence, its save/config behaviour, static Manager embedding or licenses. Covers the engine-adapter rules (adapters never narrow upstream; no per-work whitelists or enums; honest coverage when upstream supports one work), personal and organization forks, GitHub upstream pins, Cargo path patches and entry crates, the thin adapter crate or vendored C header, worker/frame-lease mapping, vcpkg dependencies and overlay ports, merging per-platform fork lines, license obligations and the GPL/AGPL static-embedding review, the contributor build/package PR gate, ABI query probes and headless acceptance. Not for a new in-repo SDK core; use shinkiemu-new-core.
---

# 把第三方引擎移植进 ShinkiEMU

外部核心 = 原样保留的上游引擎 + 一个实现 C ABI v3 的薄 adapter。贡献者先在自己的 GitHub fork 上开发 adapter；维护者审查后把它放进 `Project-Shinkirou` 组织 fork。ShinkiEMU 用精确 commit 固定组织 fork，不复制 adapter 代码。

先确认该走移植路径：上游能跑，许可证允许再分发且我们能履行义务。否则用 `shinkiemu-new-core` 基于 SDK 自研（决策表在那里的 §0）。

契约文档：[Core ABI](../../../docs/en/core-abi.md)、[Build and release](../../../docs/en/build-and-release.md)、[ThirdParty/README](../../../ThirdParty/README.md)、[THIRD_PARTY_NOTICES](../../../THIRD_PARTY_NOTICES.md)、[ORIGINS](../../../ORIGINS.md)。运行模型（worker、帧、生命周期、boundary）见 `shinkiemu-new-core` §3–§4，构建/CI/发布流程见 `shinkiemu-maintenance`。

## 引擎适配器，不是作品播放器

外部核心同样是引擎适配器：adapter 让上游引擎在 ShinkiEMU 里运行它能运行的任意作品，不负责把它收窄成某一款作品的播放器。下面七条与 [`shinkiemu-new-core`](../shinkiemu-new-core/SKILL.md#引擎适配器不是作品播放器) 写法一致，改一处要同步另一处。

1. **改动边界。** 自研核心（CMVS、Musica）可以全量修改。外部移植核心（Siglus、Sena、Ethornell、Artemis、ReMinori、Impacto、Ren'Py、KrKr、RFVP）只改 adapter 层，尊重上游行为；上游缺陷按 §3 加最小通用钩子并提交上游（§9）。
2. **定位。** 核心按引擎适配，要能运行该引擎的任意作品。代码里用到的作品名、文件名、数值都来自输入数据（原版文件、配置），不写在核心里。
3. **Probe。** 按引擎特征识别：文件格式、魔数、目录结构。不得有作品白名单，EXE 名、Steam appid、固定 hash、固定游戏 ID 都不能作为识别条件或返回值。`game_id` 由实际路径或内容推导。probe 接受的输入，open 按同一套条件处理，不在 open 里另设作品门禁。
4. **脚本和资源。** 支持引擎的多种编码和格式。入口脚本、标题图、归档集合从引擎自己的配置或启动约定读取，不写死某款作品的资源文件名或入口。启动必需资源和可选资源分开，缺可选资源的作品照样能打开。
5. **按作品变化的数据**（标题菜单、鉴赏/回想、角色、原生存档身份、画布尺寸等）按以下顺序取得，核心里不写作品枚举，也不按作品名分派数据：
   1. 先解析原版文件（脚本、系统归档、原生配置、可执行文件里的数据表），结果与原版行为一致。上游已有的解析和数据层（例如 Impacto 的 `profiles/*`）直接复用。
   2. 确实解析不了的，由 Metadata 里按发行版本绑定的配置提供。adapter 在 descriptor 里声明对应配置字段，值写在 `Metadata/engines/presets/<rid>/<plugin_id>.json`（`shinki.emu.presets.v1`，按 VNDB rID 绑定），或由补丁配方输出 profile 的 `configuration` 提供（按文件指纹识别发行，见 `shinkiemu-patch`）。adapter 只认字段，不认作品。
   3. 源文件本身不符合引擎通用格式时，用 Luau 补丁把资源修成标准形式（补丁只改资源，不改 EXE），adapter 不为它加特例。Ren'Py HimaNatsu 的 RPA chunk 顺序就由 `Metadata/patches/renpy-himanatsu.luau` 处理。
6. **存档。** 尽量兼容原版存档。外部核心沿用上游的存档位置和格式（§4）；只有确实无法兼容时才隔离，隔离按游戏路径或内容身份划分，不按作品名。
7. **兼容性。** 允许破坏性升级。去掉作品绑定后跑不起来的现有作品，用 Luau 补丁修正源文件中不标准的地方，或用配置切换（descriptor 字段加 Metadata preset），不在 adapter 里为作品写特例。删配置字段按迁移处理（§4）。

原生签名、校验和、SerialCode 绑定、真实的格式版本检查和补丁配方里的发行指纹属于引擎能力或发行识别，不算作品白名单，不要为了"通用"删掉。

### adapter 不收窄上游

上游能处理的作品、格式和 profile，adapter 原样放行。adapter 的 probe、open 校验和默认配置都不能比上游更窄，也不能用一张作品白名单替换另一张。

上游本身只支持一款作品时，如实声明覆盖范围：`docs/{zh-cn,en,ja}/cores/<slug>.md` 和 README 引擎表写明已验证的作品，兼容性记录按实际结果填写。probe 仍按引擎特征识别，adapter 不再加白名单；上游跑不了的作品由上游自己的真实检查在 open 时报错。要扩展到上游没有覆盖的作品，就在 fork 里按引擎能力补实现并尽量提交上游，不在 adapter 里加作品分支。

反例（2026-10 审计，路径前缀为 fork 仓库名）：

- **Sena：koikake.exe/appid 门禁。** adapter `sena-rs/crates/shinki-emu-sena-rs/src/config.rs` 只接受 `koikake.exe`，或 `Game.exe` 加内容为 `2242710` 的 Steam appid 文件；`lib.rs` 固定返回 `game_id=koikake`、`format=softpal-koikake`。上游 `crates/pal-vm/src/assets.rs` 已经按 File.dat/Text.dat 布局和脚本 extcall 结构区分 SoftPAL 变体。应按 PAL 结构识别，`game_id` 由路径推导，格式标识随内容变化。
- **Impacto：只允许 profile=cc。** adapter `impacto/src/shinki/core.cpp` 的 profile 枚举只有 `cc`，启动时清空上游 `Profile::GameDefinitions` 后只注册 `profiles/cc-steam/game.lua`，资源校验只认 CC 的十个 MPK，识别到固定 MPK 组合就返回 chaos-child。上游 `profiles/*` 本来就是多作品数据层，adapter 应复用上游的 profile 注册和选择。默认 `use_backup_archives=true` 偏向特定安装状态，应移到对应补丁配方。
- **ReMinori：eden.exe 白名单和固定资源。** adapter probe（`ReMinori/crates/shinki/src/lib.rs`）要求 `eden.exe`/`eden_en.exe` 并固定返回 eden；fork 加的离屏 open（`crates/engine/src/engine.rs`）强制加载 `a001.sc`、校验 `topmenu0.png`。上游 ReMinori 只实现了 eden，这正是"如实声明覆盖范围"的情形：文档写清只验证 eden，adapter 去掉 EXE 白名单和额外资源门禁。
- **Ethornell：伪造 Tayutama2TV。** `ethornell-rs/crates/ethornell-app/src/lib.rs` 在 GetGameId 附近找不到字符串比较时返回 `DEFAULT_GAME_ID = "Tayutama2TV"`，`crates/ethornell-vm/src/lib.rs` 的默认实现也写死同一标题。识别失败就冒充另一款作品，会改变脚本的身份比较结果。这段来自上游，adapter 不能把它当游戏身份用：真实 game ID 按第 5 条取得，上游缺注入口时加最小通用钩子并提交上游。`sysgrp.arc:SGTitle*` 资源名触发标题状态属于同一类问题。

正例：Siglus、Artemis、Ren'Py 的 adapter 只按引擎结构识别（Gameexe/Scene.pck、PFS 内 `system.ini`、RPA/RPYC 签名），没有作品判断；HimaNatsu 的非标准 RPA 由补丁配方修正，运行时不含该作品的特例。

## 现有例子

| 核心 | 类型 | adapter | 移动端 Rust 路径 query |
|---|---|---|---|
| rfvp | Rust | `crates/shinki-emu-rfvp`；`#[cfg_attr(feature = "dynamic-plugin-export", no_mangle)]` 门控导出 | `shinki_emu_rfvp::shinki_emu_query`（Android、iOS） |
| siglus_rs | Rust | `shinki/shinki-emu-siglus_rs`，带 `[package.metadata.shinki]` 来源块 | `shinki_emu_siglus_rs::query`（Android、iOS） |
| art3m1s-core | Rust + Luau | `crates/shinki-emu-art3m1s-core`，静态 feature `static-core` | `shinki_emu_art3m1s_core::artemis_query`（Android） |
| sena-rs | Rust | `crates/shinki-emu-sena-rs`；另有 `pub const QUERY`；ready 握手在 `session.rs` | `shinki_emu_sena_rs::shinki_emu_query`（iOS） |
| ethornell-rs | Rust | `crates/shinki-emu-ethornell-rs`；另有 `pub const QUERY` | `shinki_emu_ethornell_rs::shinki_emu_query`（iOS） |
| ReMinori | Rust, AGPL | `crates/shinki`（`build.rs`、`MODIFICATIONS.md`） | `shinki_emu_ReMinori::shinki_emu_query`（iOS） |
| impacto | C++ | `shinki/` Cargo 包装 + `src/shinki/core.cpp`（worker `Run`、`GetFrame`）；`vendor/shinki-emu-abi/include/shinki_emu.h` | `shinki_emu_impacto::shinki_emu_query`（Android，feature `static`） |
| kirikiroid2-web | C++ | `shinki/`（`build.rs`、`native/NativeSession.cpp`、`NativeOwner.cpp`）；`shinki/native/include/shinki_emu.h` | `shinki_emu_kirikiroid2_web::query`（Android，feature `static`） |
| krkr2 | C++ | `shinki/Cargo.toml`（`native-static`），从 kirikiroid2-web 移植嵌入接缝 | `shinki_emu_krkr2::query`（Android、iOS；没有桌面动态包） |
| cmvs-rs | Rust | `crates/shinki-emu-cmvs-rs`，`dynamic-plugin-export` 门控导出；与树内 `shinki.emu.cmvs` 并存 | 无（只有桌面动态包；`publish_blocked` 写明了原因） |
| renpy | C++ + Python | `shinki/core.cpp` + `shinki_renpy_bridge.py`，打包官方 Ren'Py SDK | `shinki_emu_renpy::query`（Android，feature `static`） |

从根 `Cargo.toml` 继承组织 fork 的 Git 依赖。独立构建走 `Source/Cores/<id>/` 的入口，静态 Manager 直接使用同一个实现依赖。

## 1. fork 与分支

1. 在自己的 GitHub 账号下 fork 上游，保留历史、作者和默认分支，加 `upstream` remote。adapter 从明确的上游 commit 开始，优先用上游 tag。`upstream_identity.repo` 指向原作者 GitHub 仓库。
2. 组织 fork `Project-Shinkirou/<上游仓库名>` 由维护者创建或更新，只有维护者能写入。plugin_id 保留上游仓库名和大小写（`siglus_rs`、`ReMinori`、`sena-rs`）。维护者把审查过的提交普通 push（不 force）到组织 fork 的某个分支，之后 ShinkiEMU 的 pin 才能指向它。
3. 分支名用描述用途的名字。`build/cores.toml` 的 `adapter_branch` 记录真实集成分支，Cargo 按完整 `rev` 取提交。已推送的分支不 rebase、不 force push。
4. 同一 fork 的多条平台线要合到一个共同分支时用真实 merge，保留每条线的祖先，然后在共同 head 上重跑所有目标的门禁。KrkrCommon63 就是这样把 Linux、Mac、Android 三条 kirikiroid2-web 线合到 `shinki-common63` 的。合流常见冲突：平台专用的静态 host/资源路径被误用到其他平台；同一个 helper 在共享头文件和平台文件里各定义一次。冲突按平台条件收窄，不删功能。
5. 根 `Cargo.toml` 的 `[workspace.dependencies]` 写组织 fork 的 `git` URL、完整小写 40 位 `rev` 和 `default-features = false`。由 `cargo metadata --format-version 1` 更新 `Cargo.lock`。产品清单里只登记上游身份和发布追踪字段：

```toml
[cores.attribution]
adapter_branch = "<组织 fork 分支>"
release_sequence = 1
upstream_identity = { repo = "https://github.com/<原作者>/<repo>", ref_kind = "tag", ref_name = "<上游 tag>", commit = "<上游完整 commit>" }
```

上游 commit 有 tag 时用 `ref_kind = "tag"`，否则记原上游包含该 commit 的真实分支。`cargo xtask release --component <id> --dry-run` 列出版本、标签与发布目标。提交和上游归属由维护者审查。

**本地验证。** 将个人 fork 放在仓库外，在根 `Cargo.toml` 添加以 Git URL 为表名的 `[patch."https://github.com/Project-Shinkirou/<repo>"]`，用 adapter 的实际包名和路径（[示例](../../../docs/en/build-and-release.md#work-on-a-fork-locally)）。运行不带 `--locked` 的 `cargo metadata --format-version 1`，再用 `cargo build --locked --release -p <入口包>` 验证，包括尚未提交的修改。提交产品 PR 前移除本地 patch，改回组织 fork 上已公开的 pin，并更新 `Cargo.lock`。

## 2. adapter 形态

**Rust 上游：fork 内一个薄 adapter crate。**
- fork 内新增一个实现 crate（`crates/shinki-emu-<id>` 或 `crates/shinki`），保留用于 Rust 路径调用的 `rlib`；按需保留 fork 独立构建的 `cdylib`。产品的独立入口 crate 在 `Source/Cores/<id>`，例子见 [`Source/Cores/rfvp`](../../../Source/Cores/rfvp/)。
- 用 git 依赖 `https://github.com/Project-Shinkirou/ShinkiEMU` 的 `shinki-emu-abi`（需要时加 `shinki-emu-sdk`），`[build-dependencies]` 同样引 `shinki-emu-build`，`rev` 写完整 SHA。在 ShinkiEMU 里，根 `Cargo.toml` 的 `[patch."https://github.com/Project-Shinkirou/ShinkiEMU"]` 会把它们换成树内 crate，所以 fork 里的 pin 只影响 fork 单独构建，但仍要指向真实存在的 ShinkiEMU commit。
- `build.rs` 调用 `CoreBuildManifest::new("<plugin_id>", "rust-cdylib")`，把 fork 的 `LICENSE` 和修改声明列到 `licenses/<plugin_id>/` 下。不支持的 triple 在 `build.rs` 里尽早报错。ReMinori 的 `crates/shinki/build.rs` 可作模板。

**C/C++ 上游：复制头文件 + 原生共享库。**
- 把 `Source/FamilyApi/shinki-emu-abi/include/shinki_emu.h` 复制进 fork（impacto、renpy 放 `vendor/shinki-emu-abi/include/`，kirikiroid2-web 放 `shinki/native/include/`），用 C++ 实现 `CoreApi`。
- fork 里一个小 Cargo 包（`shinki/Cargo.toml` + `build.rs`）通过 `shinki_emu_build::configure_cmake` 驱动 CMake，写 `artifact_kind = "native-shared"` 和绝对 `native_library`；Android 静态嵌入用 `native-static` 和 `link_android_static_core`。
- 升 ABI 时重新复制头文件。目前三个 fork 的头文件相同，但都比产品头文件旧，没有 `CAP_ASYNC_TEXT`/`AsyncTextServiceV1`。这些新增是向后兼容的，旧头文件仍能工作，只是看不到新能力。要用异步文本时先同步头文件。

**动态与静态 query。**
- 桌面按未修饰名 `shinki_emu_query` 加载。移动端 Manager 链接 `rlib` 并调用 Rust 路径；两个核心都导出未修饰 `shinki_emu_query` 会在同一个二进制里冲突。
- 未修饰导出由独立入口或独立 feature 门控（RFVP 用 `dynamic-plugin-export`），Manager 的静态依赖不启用它。
- 给 `STATIC_CORES` 提供 Rust 路径入口：函数本身、`pub const QUERY: Query = shinki_emu_query;`，或具名函数（`artemis_query`、`siglus_rs::query`）。
- `query` 先把输出置空，版本不是 `ABI_VERSION` 时返回 `UNSUPPORTED`，不创建任何原生状态。

## 3. 把上游主循环改成会话

多数引擎自带 `loop { poll events; update; render; present }` 和一个窗口。adapter 要把它改成会话：

- **每个会话一个 worker 线程** 跑上游循环。`open` 启动它，用有界 `recv_timeout` 等 ready 信号，每一步检查取消。sena-rs 的 `session.rs` 是模板：`sync_channel(1)` 握手；`Disconnected` 时 join worker 并返回它记录的错误；取消返回 `CANCELLED`。C++ 里 impacto 的 worker 是 `core.cpp` 的 `Run`。
- **窗口换成离屏输出。** 渲染到离屏目标，回读到不可变 CPU 缓冲，递增 generation 后发布。ReMinori 用三个不可变槽，借出的租约不会卡住 VM 或渲染。`frame` 返回 `CpuLease`（impacto 的 `GetFrame`）。
- **input/control/state** 变成发给 worker 的命令。暂停是停上游时钟，不是阻塞线程。
- **panic 和异常** 不跨 ABI。每个导出和 worker 主体用 `catch_unwind`（Rust）或 `try/catch`（C++）包住，转成带消息的 `SESSION_FAILED`，`close` 仍要 join 并释放所有资源。
- **窗口和模态框** 走宿主的 window/modal 服务，引擎不能自己开。
- 保留上游的 VM、调度、合成、混音、菜单和存档代码，adapter 只做转换。上游需要钩子时（离屏目标、可取消的等待），加最小的通用钩子并尝试提交上游（§9）。

## 4. 存档、key、配置：最小改动

- 外部引擎保持上游自己的存档、key、配置行为、位置和格式，AppData/Documents 也一样，不要重定向到 `.shinki`。只有自研引擎（CMVS、Musica）用 `<game>/.shinki/<core>/`。
- adapter 自己在游戏目录写的文件放 `<game>/.shinki/` 下。
- 不要加托管存档根、游戏目录重叠检查、必填的 `save_root`/`save_directory` 字段或自造的 AppData 命名空间。ReMinori 删掉了 adapter 专用的 `save_directory`，改用上游的 `userdata/` 解析，只是把可执行文件目录换成游戏目录（`ReMinori/MODIFICATIONS.md`，2026-10-29）。
- 收尾前对比 fork base 和上游的存档/key/配置路径，剩下的每处差异都在修改声明里写明原因。
- 删配置字段是一次迁移：未知字段 id 的存储值会导致启动失败，Manager 会清理退役 id。旧的 adapter 专用路径迁移一次（不覆盖已有文件），之后只读上游路径。

## 5. 原生依赖

- 由 adapter 的 Cargo build script 准备锁定的原生输入。沿用 `shinki-emu-build` 的 vcpkg、下载和 CMake 辅助函数；共享原生库优先依赖现有 `links` crate，不重复构建 FFmpeg 或 libass。
- fork 的 overlay port 放在 `vcpkg/ports/<port>/`。修 port 时改 patch 或 portfile，不关闭配置校验、不伪造 probe 结果（见 `shinkiemu-maintenance` §6）。
- 原生运行库、资源和许可通过 Cargo 构建脚本交给打包工具；系统开发包和 SDK 由构建机安装。

## 6. 注册与嵌入

- `Source/Cores/<id>` 的 Cargo 入口用 `workspace = true` 引用 fork adapter。Rust 入口输出 `cdylib`；驱动 CMake 的入口依赖原生 adapter 产出共享库。`build/cores.toml` 里加 `[[cores]]`，每个目标列入 `supported_targets` 或 `[[cores.blocked]]`，许可用 fork 相对的 `adapter_licenses` 或产品相对的 `licenses`。字段规则见 `shinkiemu-new-core` §6。
- 静态嵌入在 `static_manager_targets`、Manager `Cargo.toml` 的目标依赖和 `static_cores.rs` 的 `STATIC_CORES` 三处对齐。Manager 继承同一个 workspace pin，按需开启静态 feature。只支持某个 ABI 时用精确 triple 表，例如 `[target.aarch64-linux-android.dependencies]`，不要和覆盖它的 `cfg` 表重复声明。
- 静态核心与 Manager 共享依赖图：kira（根 `[patch.crates-io]` 指向 `Project-Shinkirou/kira`）、mlua/mlua-sys、cpal、wgpu 必须统一。两个版本的 `links =` crate 会让解析失败。改 fork 对齐，不要再加 patch。Artemis 在所有平台都用 Luau，Android 嵌入时共用 Manager 的运行时。
- Android 嵌入 C++ 核心时：Java 方法名不能和 SDK 父类冲突（Krkr 把 `getDeviceId` 改成 `getAndroidDeviceId`），整个 APK 只有一个 `JNI_OnLoad` 和一个 Activity，资源上限遵守 Manager 当前契约。

## 7. 许可证与公开源码

- 动手前确认上游许可允许再分发修改后的源码和二进制，贡献者有权按该许可提交 adapter 代码。许可不清楚或不允许再分发的上游走 `shinkiemu-new-core` 自研。
- 保留上游 `LICENSE`、署名和 notice。fork 里加修改声明（`MODIFICATIONS.md`），写明上游 URL、base commit、adapter 改了什么和原因、对应源码在哪发布。
- fork 的许可路径写入 `adapter_licenses`，产品侧文件写入 `licenses`，并在 `THIRD_PARTY_NOTICES.md`、`ORIGINS.md` 里登记。原生依赖的许可由 build script 提供。
- 每个外部核心的公开源码是 `Cargo.lock` 记录的公开 fork 提交；发布前确认读者可获取，并保留适用的对应源码、重链接和声明义务。
- 第三方特殊条款（FMOD、Spine、MS PGothic 等）原文保留并单独说明，不改标成 MIT。
- 许可或声明义务未满足时设 `publish_blocked = "<原因>"`，这样的核心不会发布。
- GPL/AGPL 核心默认作为独立核心分发。静态嵌入会把它和 MIT/Apache 的 Manager 链接到同一二进制，公开发布前需由维护者针对该核心单独评审许可与对应源码义务，将结论写进发布说明。ReMinori（AGPL）的 iOS 静态集成遵循同一规则。
- 商业游戏数据、正文、key、私有配置和存档不能提交。测试用合成 fixture。

## 8. pin 升级

步骤和 `Cargo.lock` 规则见 `shinkiemu-maintenance` §5。要点：

1. fork 改动提交并非 force 推送，取完整 SHA。
2. 在根 `Cargo.toml` 更新该 fork 的 `rev`，运行 `cargo metadata --format-version 1` 更新 `Cargo.lock`。入口和 Manager 继承同一个依赖。
3. 核对 `git diff -- Cargo.lock`，保留预期的提交和依赖变化。运行 `cargo metadata --locked --format-version 1`，不要批量更新其他依赖。
4. 要发布新版本时 `release_sequence` +1，序号由发布 owner 确认。

## 9. 回馈上游

- 与 ShinkiEMU 无关的修复（VM bug、格式边界、可移植性）从基于 `upstream/main` 的 `upstream/<topic>` 分支提 PR，附合成测试并跑上游自己的检查。
- adapter 专用改动（ABI 胶水、离屏输出、打包）留在 fork，写进修改声明。
- 上游合入后，fork 下次跟进上游时删掉自己的副本。

## 10. PR 门禁与验收

贡献者 PR 的门禁与 `shinkiemu-new-core` §11 相同：核心在 Windows x64 和 Android ARM64 中声明支持的目标都跑通 Cargo release 构建和 `package`。Cargo 测试、ABI 探针、headless 回放可选；把实际命令和结果写在 PR 里，不提交截图、日志、hash 或运行产物。

发布前由维护者验证：所有支持目标在同一 commit 上通过 CI；独立进程可通过 ABI query 探针；许可随包，锁定的公开源码可获取；使用合法持有的游戏以 headless runner 打开并出帧。

ABI 探针要在独立原生进程里加载打包后的库，不要用 Python ctypes：Linux 上 Python 的 hashlib 会先加载系统 libcrypto，和核心自带的 OpenSSL 冲突；Windows 上 ctypes 持有 DLL 会让临时目录删不掉。用 `Tools/krkr-query-probe.c`（名字带 krkr，但对任何核心通用）：用 pin 的 `shinki_emu.h` 编译，参数是打包后的库路径，`LoadLibraryExW`/`dlopen` 加载后检查 ABI 3 返回完整表、其他版本返回 `UNSUPPORTED`、空输出指针返回 `INVALID_ARGUMENT`。

## 完成检查

- [ ] adapter 从个人 fork 开发，由维护者更新组织 fork。保留上游历史，设好 `upstream` remote，记录 base commit 与真实 `adapter_branch`。
- [ ] 根 `Cargo.toml` 和 `Cargo.lock` 固定组织 fork 的完整 SHA，`Source/Cores/<id>` 继承依赖。`[cores.attribution]` 记录 GitHub 上游身份与序号；`release --dry-run` 列出的版本、标签与目标符合预期。
- [ ] GPL/AGPL 核心如要静态嵌入，维护者已完成单独的许可和对应源码评审。
- [ ] Manager 静态依赖继承同一个 workspace pin，只开启所需的静态 feature；`STATIC_CORES` 已更新。
- [ ] adapter 保留上游 VM、渲染、音频和存档，只加 ABI 胶水和最小钩子。
- [ ] 未修饰 `shinki_emu_query` 只在动态构建导出；静态构建有 Rust 路径 query。
- [ ] C/C++：复制的 `shinki_emu.h` 覆盖所用的 ABI 能力。
- [ ] panic/异常被拦住；`open` 可取消；`close` join worker；借出的租约不阻塞推进。
- [ ] 存档/key/配置路径与上游一致，差异在修改声明里说明。
- [ ] 原始许可和声明已登记并随包；锁定的 fork 提交可公开获取。
- [ ] `Cargo.lock` 只含此次 pin 和实际依赖变更；未提交本地 patch。
- [ ] `docs/{zh-cn,en,ja}/cores/<slug>.md` 链接上游项目；README 引擎表已更新。
- [ ] §10 验收全部满足。
- [ ] adapter 的 probe、open 校验和默认配置不比上游窄；上游只覆盖一款作品时，文档如实写明覆盖范围。
- [ ] probe 只按引擎特征识别，没有 EXE 名、Steam appid、固定 hash、固定游戏 ID 白名单；probe 和 open 的接受条件一致。
- [ ] adapter 里没有作品枚举，也没有按作品名分派的数据或资源名。
- [ ] 按作品变化的数据按"原版文件解析 → Metadata 按发行版本配置 → Luau 补丁修正非标准源文件"的顺序取得。
- [ ] 存档兼容原版（沿用上游）；只在确实无法兼容时隔离。
- [ ] probe/open 用至少两款不同作品，或模拟不同归档集合、入口名、目录名、EXE 名的合成 fixture 验证过。
