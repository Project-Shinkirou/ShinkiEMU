---
name: shinkiemu-new-core
description: Use when writing a new ShinkiEMU core from scratch on the in-repo Rust SDK (a Source/Families implementation and Source/Cores entry crate), when changing an in-repo core (CMVS, Musica), or when choosing between writing one and porting a third-party engine. Covers the engine-adapter rules (engine-trait probe without per-work whitelists or enums; per-work data from original files, then release-bound Metadata, then Luau patches; original-save compatibility), the port-vs-SDK decision, crate layout, Cargo.lock reconciliation, query/CoreApi table, worker, CPU frame leases and pending-frame identity, SDK modules, build/cores.toml registration and family_id, game recognition, static embedding, Cargo builds and tests, docs, release tags, the contributor PR gate (Windows x64 and Android ARM64 build and package) and maintainer release acceptance (ABI probe, exact-head CI, licenses, real game opens and renders headless). Not for wrapping an existing third-party engine; use shinkiemu-integrate-existing-core.
---

# 基于 SDK 编写新的 ShinkiEMU 核心

参考实现是 CMVS（`Source/Families/shinki-emu-cmvs`），Musica（`Source/Families/shinki-emu-musica`）是第二个例子。照着它们的结构写，不要另起一套约定。Musica 的存档身份和标题/鉴赏/角色数据是作品绑定的反例，见下一节。

契约文档直接链接，不要抄进核心：[Core ABI](../../../docs/en/core-abi.md)、[Build and release](../../../docs/en/build-and-release.md)、[Metadata](../../../docs/en/metadata.md)、[SDK README](../../../Source/SDK/shinki-emu-sdk/README.md)。构建、CI、发布流程见 `shinkiemu-maintenance`。

示例在 `Source/Examples/shinki-emu-tilefix`（package `shinki-emu-tilefix`，ID `shinki.example.tilefix`，family `tilefix`）。它用 SDK 实现 query/probe/open/input/frame/close，运行时生成自造数据，不含商业资源。示例不在官方目录，直接用 Cargo 构建和测试即可，命令见 §9。

## 引擎适配器，不是作品播放器

一个核心对应一个引擎，目标是运行这个引擎做出的任意作品。Musica 核心是 Musica 引擎的适配器，eden 只是它的验收样本之一。下面七条在本 skill 和 `shinkiemu-integrate-existing-core` 里写法一致，改一处要同步另一处。

1. **改动边界。** 自研核心（CMVS、Musica，以及按本 skill 新写的核心）可以全量修改，VM、存档、probe、前端都能重构。外部移植核心（Siglus、Sena、Ethornell、Artemis、ReMinori、Impacto、Ren'Py、KrKr、RFVP）只改 adapter 层，尊重上游行为，细则见 `shinkiemu-integrate-existing-core`。
2. **定位。** 核心按引擎适配，要能运行该引擎的任意作品。代码里用到的作品名、文件名、数值都来自输入数据（原版文件、配置），不写在核心里。
3. **Probe。** 按引擎特征识别：文件格式、魔数、目录结构。不得有作品白名单，EXE 名、Steam appid、固定 hash、固定游戏 ID 都不能作为识别条件或返回值。`game_id` 由实际路径或内容推导。probe 接受的输入，open 按同一套条件处理，不在 open 里另设作品门禁。
4. **脚本和资源。** 支持引擎的多种编码和格式。入口脚本、标题图、归档集合从引擎自己的配置或启动约定读取，不写死某款作品的资源文件名或入口。启动必需资源和可选资源分开，缺可选资源的作品照样能打开。
5. **按作品变化的数据**（标题菜单、鉴赏/回想、角色、原生存档身份、画布尺寸等）按以下顺序取得，核心里不写作品枚举，也不按作品名分派数据：
   1. 先解析原版文件（脚本、系统归档、原生配置、可执行文件里的数据表），结果与原版行为一致。
   2. 确实解析不了的，由 Metadata 里按发行版本绑定的配置提供。核心在 descriptor 里声明对应配置字段，值写在 `Metadata/engines/presets/<rid>/<plugin_id>.json`（`shinki.emu.presets.v1`，按 VNDB rID 绑定），或由补丁配方输出 profile 的 `configuration` 提供（按文件指纹识别发行，见 `shinkiemu-patch`）。核心只认字段，不认作品。
   3. 源文件本身不符合引擎通用格式时，用 Luau 补丁把资源修成标准形式（补丁只改资源，不改 EXE），核心不为它加特例。Ren'Py HimaNatsu 的 RPA chunk 顺序就由 `Metadata/patches/renpy-himanatsu.luau` 处理。
6. **存档。** 尽量兼容原版存档：原版位置和字节格式，原版可执行文件能读（自研核心见 §4 存档，外部核心沿用上游）。只有确实无法兼容时才隔离，隔离按游戏路径或内容身份划分，不按作品名。没有已知原生格式的作品也要真实持久化，不能返回成功却不写入。
7. **兼容性。** 允许破坏性升级。去掉作品绑定后跑不起来的现有作品，用 Luau 补丁修正源文件中不标准的地方，或用配置切换（descriptor 字段加 Metadata preset），不在核心里为作品写特例。删配置字段按迁移处理（§4 配置）。

有几类东西看起来像作品绑定，实际属于引擎能力，不要为了"通用"删掉：原生签名、校验和、SerialCode 绑定、真实的格式版本检查；补丁配方里的发行指纹；格式版本覆盖范围（CMVS 只挂载 CPZ5 是版本覆盖，不是白名单，如实写进文档即可）。

正例：CMVS 按 `CPZ5` magic 识别，游戏 ID 取目录名，归档和密钥由 profile 提供；Siglus 由 Gameexe/Scene.pck/Select.ini 驱动，加密表作为输入传入；Artemis 读取 PFS 内 `system.ini`，依次尝试 UTF-8/Shift-JIS/GB18030。

### 反例：Musica（2026-10 审计）

文件都在 `Source/Families/shinki-emu-musica/src/` 下（`shinki-emu-musica-keys` 是同目录的兄弟 crate）。

- **存档身份枚举。** `minori_data.rs` 的 `MinoriTitle` 只有 `EdenEnglish`、`Perseus`，靠 `eden_en.exe`、`perseus.exe` 识别。其他作品不建原生目录，`native_directory/slots.rs` 拒绝槽位操作（"native slots require an identified original game executable"），全局变量、CG、已读记录的写入只打警告并返回成功。违反第 5、6 条：原生存档身份、文件命名和格式参数应先从原版文件解析（注册表 SerialCode 前缀、容器签名），解析不了再走 Metadata 配置。
- **标题 CLEAR 标记。** `runtime/title.rs` 用 `TOHKA_CLEAR`、`AYAME_CLEAR`、`SUI_CLEAR`、`REN_CLEAR` 决定菜单变体，点击区固定在 1280 宽画布右侧。换作品后菜单解锁规则不变。
- **鉴赏脚本表。** `runtime/gallery.rs` 内置 `fb_ren_04.sc` 等回想脚本、`fb_aya_12.sc` 等影片脚本和 `ed_ayame.avi` 等文件名。其他作品的回想和影片入口会访问不存在的资源。
- **角色前缀。** `voice_preferences.rs` 把 ren/sui/aya/tou/mot 文件名前缀解释为五个固定角色，配置界面表达不了其他作品的角色集合。
- **归档布局当成引擎条件。** `native.rs` 的 probe 要求 bg/bgm/scr/st/sys/se/voice/mov 八个 PAZ 同时存在，`shinki-emu-musica-keys` 要求密钥表恰好包含这八个角色。归档角色应由实际 profile 声明，并区分必需和可选。

## 0. 先选路径

| 情况 | 路径 |
|---|---|
| 有可运行的上游实现，许可证允许再分发（MPL、MIT、Apache，或能履行源码义务的 GPL/AGPL；GPL/AGPL 静态嵌入 Manager 另需维护者评审，见 `shinkiemu-integrate-existing-core` §7） | 移植第三方 fork，用 `shinkiemu-integrate-existing-core` |
| 上游存在但许可证不兼容、无法分发，或代码无法在离屏/多平台下运行 | 用本 skill 基于 SDK 自研 |
| 没有上游实现，只有原版可执行文件 | 用本 skill，逆向原版行为后自研（CMVS、Musica 都是这样做的） |
| 上游只覆盖部分格式 | 先移植上游；缺的部分作为 fork 内的 adapter 修改并尽量提交上游，不要在树内再写一个同引擎核心 |

自研核心属于本仓库，plugin_id 用 `shinki.emu.<family>`，版本用 SemVer。

## 1. 命名

- **plugin_id**：`Descriptor.plugin_id`、`core-build.json`、`build/cores.toml` 的 `id`、catalog 和存档共用的稳定标识，格式 `shinki.emu.<family>`。字符限 `[A-Za-z0-9._-]`，最长 128 字节（`CoreBuildManifest::validate`，`Source/Tools/shinki-emu-build/src/lib.rs:54-62`）。发布后不能改名。
- **slug**：由 plugin_id 推导，不手写。小写后把 `.`、`_` 换成 `-`（`shinki.emu.cmvs` → `shinki-emu-cmvs`）。docs 页名、tag `core-<slug>-v<semver>` 和归档名都用它。重复 slug 会被 `Inventory::validate` 拒绝。
- **crate**：实现位于 `Source/Families/shinki-emu-<family>`（package `shinki-emu-<family>`）；独立入口位于 `Source/Cores/shinki.emu.<family>`（package `shinki-emu-core-shinki-emu-<family>`）。把实现加入根 `Cargo.toml` 的 `[workspace] members`，入口由 `Source/Cores/*` 纳入。
- **family_id**：引擎族 ID，`build/cores.toml` 的必填字段，与 descriptor 里的族字符串一致（`"cmvs"`）。格式是 1–64 个小写 ASCII 字母、数字或连字符，以字母开头（`xtask/src/cores.rs`）。Manager 的目录识别按它把游戏关联到核心。多个核心可以共用一个 family，例如 `ReMinori` 和 `shinki.emu.musica` 都是 `musica`。

## 2. crate 骨架

`Cargo.toml` 参照 `Source/Families/shinki-emu-cmvs/Cargo.toml`：

```toml
[package]
name = "shinki-emu-<family>"
version.workspace = true
edition.workspace = true
license.workspace = true
build = "build.rs"

[lib]
crate-type = ["rlib", "cdylib"]   # 参照 CMVS；独立产物从入口 crate 构建

[dependencies]
shinki-emu-abi = { path = "../../FamilyApi/shinki-emu-abi", features = ["diagnostic-bridge"] }
shinki-emu-sdk = { path = "../../SDK/shinki-emu-sdk", default-features = false, features = ["runtime"] }

[build-dependencies]
shinki-emu-build = { path = "../../Tools/shinki-emu-build" }
```

参照 [`Source/Cores/shinki.emu.cmvs`](../../../Source/Cores/shinki.emu.cmvs/) 添加薄入口：`crate-type = ["cdylib"]`，通过相对路径依赖实现并开启 `dynamic-plugin-export` 和所需的原生 feature，引用实际 query 以链接导出。静态 Manager 直接依赖实现，不开启独立导出 feature。

`build.rs` 参照 `Source/Families/shinki-emu-cmvs/build.rs`：

```rust
use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for input in ["HOST", "TARGET", "PROFILE"] {
        println!("cargo:rerun-if-env-changed={input}");
    }
    println!("cargo:rerun-if-changed=src");
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR")?)
        .join("../../..").canonicalize()?;
    let mut manifest = CoreBuildManifest::new("shinki.emu.<family>", "rust-cdylib")?;
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        manifest.licenses.push(BuildAsset { source: root.join(file), dest: format!("licenses/shinki.emu.<family>/{file}") });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
```

`CoreBuildManifest::validate`（`lib.rs:50-105`）的规则：
- `rust-cdylib` 不带 `native_library`；`native-shared` 要求存在的绝对路径；`native-static` 要求 `.a`/`.lib`。
- 资产 source 是存在的绝对路径；dest 以 `Resources/` 或 `licenses/` 开头，不含 `\`、`:`、空段、`.`、`..`。
- 随包的每个许可文件也要列进 `build/cores.toml` 的 `licenses`。

`build.rs` 将实际打包输入写入 `OUT_DIR`。需要原生库时，由构建脚本调用 `shinki-emu-build` 或依赖共享的原生库 crate；不递归调用 Cargo，不发布。

添加 workspace 成员或依赖后，先不带 `--locked` 运行 `cargo metadata --format-version 1`，再核对 `git diff -- Cargo.lock`，只提交本次改动需要的锁文件变化。不要为此批量更新依赖。日常编译、测试和 `cargo xtask` 都用锁定的依赖。

## 3. C ABI 入口：`src/native.rs`

按顺序读 CMVS 的这些位置：

| 内容 | 位置 |
|---|---|
| 每个导出外层的 panic/错误边界 | `Source/Families/shinki-emu-cmvs/src/native.rs:28`（`boundary`） |
| 带 `Header` 的静态 `CoreApi` 表 | `native.rs:52` |
| 静态注册用的 Rust 路径 query，和动态加载用的未修饰 `shinki_emu_query` | `native.rs:69`、`native.rs:81` |
| `initialize`、`describe`、`probe`（probe 在返回前 join 自己的 worker） | `native.rs:87-202` |
| `open`：校验请求，启动 worker，等 ready 握手 | `native.rs:203-332` |
| `input`、`control`、`state`：只排队命令，不阻塞在 VM 上 | `native.rs:333-412` |
| `frame`：`shinki_emu_sdk::native::lease_cpu` | `native.rs:413-461` |
| `cancel`（只置标志）和 `close`（join worker） | `native.rs:462-487` |
| descriptor、能力位和配置 schema | `src/native/config.rs:16`（从 `crate::family::cmvs_descriptor().configuration` 生成） |
| 从 `OpenRequest` 解出配置 | `src/native/config.rs:136`（`shinki_emu_sdk::native::config_entries`） |
| worker 主循环 | `src/native/worker.rs:143-243` |

最小形态：

```rust
static API: abi::CoreApi = abi::CoreApi {
    header: abi::Header::of::<abi::CoreApi>(),
    initialize: Some(initialize), describe: Some(describe), probe: Some(probe),
    open: Some(open), input: Some(input), control: Some(control), state: Some(state),
    frame: Some(frame), cancel: Some(cancel), close: Some(close), extension: Some(extension),
};

/// 移动端 Manager 通过 Rust 路径调用。
pub unsafe extern "C" fn <family>_query(version: u32, out: *mut *const abi::CoreApi) -> abi::Status {
    if out.is_null() { return abi::INVALID_ARGUMENT; }
    unsafe { out.write(std::ptr::null()) };
    if version != abi::ABI_VERSION { return abi::UNSUPPORTED; }
    unsafe { out.write(&API) };
    abi::OK
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(version: u32, out: *mut *const abi::CoreApi) -> abi::Status {
    unsafe { <family>_query(version, out) }
}
```

`query` 不创建任何原生状态，不做 I/O，不启动线程，不装 logger。每个导出都包在 `boundary` 里，panic 变成 `PANICKED` 加一个 `Error`，绝不跨 `extern "C"` 展开。

## 4. 运行模型

- **worker 拥有引擎。** `open` 启动一个线程，VM、渲染、音频都归它。API 调用通过 channel 发 `Command`。CMVS 的循环（`worker.rs:154` 起）用 `recv_timeout(8ms)` 收命令，处理 control/input，按实际经过的时间推进（暂停时为 0），然后发布一帧。Manager 没有 advance 回调，推进完全由核心自己负责。
- **帧。** 在锁里发布不可变像素缓冲，只有内容变化时递增 `generation`（`worker.rs:231`）。第一帧之前 `frame` 返回 `FRAME_PENDING`，调用方身份相同时返回 `FRAME_UNCHANGED`，否则用 `lease_cpu` 返回 CPU 租约。被借出的帧不能阻塞 VM，也不能被覆盖；会话关闭后租约仍可读、可归还。
- **帧身份。** 每个 `frame` 结果都由核心填写真实身份，Pending 也一样：`session_epoch` 是 `OpenRequest.session_epoch`，`output_epoch` 是 `open` 返回的非零值，`reserved` 为 0。Pending 的 generation 在第一帧前可以是 0，租约字段全空。SDK 没有构造 Pending 帧的 helper；`lease_cpu` 只造租约，三个租约未归还时返回 `None`，这时同样返回带真实身份的 Pending。全零身份的 Pending 会被宿主的 `FrameSurface::validate_cpu` 拒绝，headless 报 `invalid CPU surface`。契约见 [Core ABI](../../../docs/en/core-abi.md#cpu-surface-leases)。
- **生命周期。** 状态是 `RUNNING`/`PAUSED`/`FINISHED`/`FAILED`。`cancel` 只设 `ServiceContext` 的取消标志，必须能唤醒阻塞中的原生等待。`close` 无论成败都消耗句柄，join worker 并释放所有媒体/音频资源。worker 清理前调用 `services.invalidate()`（`worker.rs:123`）。
- **窗口/模态请求** 走 `shinki_emu_sdk::native::window_request`（`worker.rs:210`）。核心不能自己开系统窗口。
- **配置。** 字段只在 family descriptor 里声明一次。Manager 会拒绝未知字段 id 的存储值，所以删字段等于一次迁移。
- **日志。** 打开 `shinki-emu-abi/diagnostic-bridge`，用 `tracing` 输出稳定的 `event = "shinki.emu.<family>.<event>"`。正文、私有路径、未审查的 Debug 内容不能进日志。
- **存档。** 自研核心按原引擎的位置和字节格式写原生存档，让原版可执行文件也能读。原引擎放在注册表、AppData、Documents 里的东西放到 `<game>/.shinki/<family>/`（CMVS：`<game>/.shinki/cmvs/registry.json`）。ShinkiEMU 在游戏目录写的其他文件也都放在 `<game>/.shinki/` 下（[Build and release](../../../docs/en/build-and-release.md#game-local-on-disk-layout)）。
- **音频设备。** 核心自己打开真实设备；设备失败就上报错误，不退回静音或空输出。要支持 headless 的 `--audio-device nullaudiodevice`，在 descriptor 里声明枚举字段 `headless_audio_device`，选项含 `nullaudiodevice`，收到该值时换成按正常时钟消费 PCM 的空输出。不声明时 runner 直接以退出码 1 拒绝这个参数（[Headless](../../../docs/en/headless.md#core-acceptance-and-captures)）。

## 5. SDK 模块怎么选

SDK 只提供公共机制，不提供游戏运行时。VM、混音规则、媒体时钟、存档和游戏规则都留在核心里。详细 API 见 SDK README。

| 需求 | 模块 / feature | 入口 |
|---|---|---|
| 进程内配置、输入、窗口命令、错误、帧值 | `runtime` | `shinki_emu_sdk::runtime::*`；C ABI 配置先过 `shinki_emu_abi::validate_config` |
| 取消、epoch、宿主服务失效、窗口结果 | `runtime`（`native`） | `native::ServiceContext`（`is_cancelled`/`invalidate`/`cancel`）、`native::config_entries`、`native::window_request`、`native::NativeFailure` |
| CPU 帧租约 | `runtime`（`native`） | `native::lease_cpu`：独立 owner 持有不可变像素和租约计数 |
| 把 C 输入复制成有序事件，指针按舞台逆映射 | `runtime`（`native_input`） | `native_input::translate`；不调用 VM |
| 逻辑舞台到光栅的映射、逻辑/物理纹理分离 | `image` | `StageCanvas`、`TextureAsset` |
| 字体排版和字形资源 | `text` | `TextScene`（`frame` 产出先资源后绘制的 `SceneCommand`）、`TextScene::layout_shared`（只读度量） |
| 有界纹理缓存、场景纹理驻留 | `image` | `TextureCache`、`SceneTextureResidency`（每个 renderer 一个实例，提交前 `prepare`） |
| 完整解码一段音频为浮点 PCM | `audio` | `decode_audio`，借用 `&[u8]`，调用方给最大帧数和取消标志 |
| 核心自己的音频设备线程 | `device-audio` | `NativeAudioOutput`、`NativeAudioConverter`（CPAL；设备错误上报，不切静音后端） |
| ASS 字幕 | `subtitle` | `AssScene`：`update` 产出资源变更，`append_draws` 追加绘制 |
| FFmpeg 视频解码 | `video-ffmpeg` | `VideoFramePacket`（BGRA8 + 实际颜色转换） |
| 归档索引和读取结果 | `archive` | `ArchiveManifest`（`shinki.emu.archive_manifest.v2`） |
| 有界明文磁盘缓存 | `cache` | `CacheIdentity`；是否落盘由核心决定 |
| 游戏目录内或外置私有配置 | 默认 | `read_game_profile`、`read_configured_profile_bytes`、`resolve_game_file` |

渲染失败要关闭对应会话，不能丢掉资源命令继续画。不要再加第二套音频栈、第二套纹理缓存或第二个 renderer。

## 6. 注册核心

`build/cores.toml` 是官方核心目录的来源：Manager 构建时从它生成内置的 `static_core_catalog`，加一条 `[[cores]]` 就是申请一个新的官方 plugin ID，由维护者决定是否接收，接收后要随新的 Manager 一起发布。参照 `shinki.emu.cmvs` 写：

```toml
[[cores]]
id = "shinki.emu.<family>"
family_id = "<family>"
display_name = "Shinki <Family>"
family_name = "<Family>"
license = "MIT OR Apache-2.0"
resources = []
licenses = ["LICENSE-MIT", "LICENSE-APACHE"]
supported_targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-linux-android"]
publish_blocked = ""
[[cores.blocked]]
target = "aarch64-apple-darwin"
reason = "<具体原因>"
# build/targets.toml 里其余每个 triple 各一条 [[cores.blocked]]
```

清单要求每个 triple 恰好出现在 supported 或 blocked 中一次，blocked 原因非空；`family_id` 必填且格式合法。`publish_blocked` 在许可或声明义务未满足时填写具体原因，这样的核心不会发布。新核心不要改 `build/targets.toml`。

`supported_targets` 只写真实构建成功过的目标：你本地跑过的 Windows x64 和 Android ARM64，PR CI 会构建的 Linux x64，或维护者 dispatch 的 `ci.yml` 跑过的其他目标。没构建过的目标写 `[[cores.blocked]]`，原因写到能定位的程度（缺哪个依赖准备路径、哪个文件哪段代码、还没有哪次构建），不要写"暂不支持"。平台按 Windows x64 → Android ARM64 → Linux x64 → macOS → iOS → OHOS 的顺序推进。

### 游戏识别与手动选择

当前代码里，Manager 扫描只用 `Source/Manager/shinki-emu-manager-core/src/detection/` 的代码检测器，覆盖 [Core ABI](../../../docs/en/core-abi.md#directory-recognition) 表里的十个族。`family_id` 没有检测器的新核心不会被关联到任何游戏，`cargo xtask matrix` 也不检查这一点。新族可以在检测器里按引擎结构加规则（同一个 PR 附合成 fixture），或者等下面的声明式识别合入。

声明式识别和手动选择正在实现，下面是计划中的形式，字段名和限制以合入后的代码为准；在此之前 xtask 和 Manager 都不认识 `recognition` 字段：

- `[[cores]]` 写 `recognition = [{ path = "data/engine.id", magic = "53484b31", offset = 0 }]`，打包时写进 `core.json` 的同名字段。至多 4 条，任意一条命中即可；`path` 是相对游戏目录的精确文件路径，按 ASCII 忽略大小写比较，最多 4 段、256 字节；`magic` 可选，是 1–32 字节的 hex，`offset` 加长度不超过 4096，没有 `magic` 时不能写 `offset`。示例里的路径和 magic 只演示格式，不是任何引擎的真实标记。不写这个字段时沿用现有检测器。
- 只有内置官方目录里的核心参与识别和手动选择，目录外或侧载的第三方核心不参与。扫描只读文件，不加载原生库，不调用 probe。
- 识别不了的游戏，用户可以从已安装且允许的官方核心里手动选；启动后出现第一个有效帧，这个选择才会保存。probe 或 open 失败时显式报错，不自动换别的核心。
- 发布影响：Windows 上已在目录里的核心，规则随新核心包更新，不用重新发布 Manager；新的官方 plugin ID 和 Android 静态核心的规则变化都要重新发布 Manager。`core.json` 拒绝未知字段，旧版 Manager 会拒绝带这个字段的包，这是破坏性变化。本地安装的官方核心 ZIP 必须与已发布官方 release 的大小、SHA256 和 manifest 身份一致。

## 7. Android / iOS / OHOS 静态嵌入

采用静态嵌入的目标需要让以下三处一致：

1. `build/cores.toml`：用 `static_manager_targets` 列出 triple。
2. `Source/Manager/shinki-emu-manager/Cargo.toml`：覆盖整个平台时用 `cfg(target_os = "android")`、`cfg(target_os = "ios")` 或 `cfg(target_env = "ohos")`；只覆盖个别 ABI 时用精确 triple 表，例如 `[target.aarch64-linux-android.dependencies]`。一个核心在同一 triple 上只声明一次。用 `path` 依赖实现、设置 `default-features = false` 并开启实际需要的静态 feature。
3. `Source/Manager/shinki-emu-manager/src/static_cores.rs` 的 `STATIC_CORES`：例如 `("shinki.emu.musica", shinki_emu_musica::musica_query)`。Manager GUI 和 Manager 的 `headless` 子命令共用这张表；独立的 `shinki-emu-headless` 按路径加载核心包。

打包会把 `embedded-cores.json` 和嵌入核心的许可写进 Manager 包。是否把核心嵌入官方 Manager 由维护者决定：它改变 Manager 的依赖图，正式 Android APK 要两个 ABI 的 release 构建，并随 Manager 发布。

## 8. 测试

- 单元和行为测试放在实现旁边（CMVS 是 `src/native/tests.rs`），用 `cargo test --locked -p shinki-emu-<family>` 跑。
- `native/tests.rs` 里的 `queued_input_drives_real_session_and_cpu_lease_survives_joined_close` 是 ABI 层行为测试的模板：真实 worker、真实输入、窗口服务替身、关闭后租约仍可读。
- 覆盖使用方看得到的行为：格式和预算边界、损坏或超大的输入、存档往返和完整性、open 期间取消、暂停/恢复、过期帧身份、租约在 worker 结束后仍有效、失败后 worker 正常退出。
- 对 `frame` 的每个返回值调用 `FrameSurface::validate_cpu(session_epoch)`，Pending 也要过。只检查状态码的测试抓不到全零身份的 Pending，打包后的核心会在 headless 里以 `invalid CPU surface` 失败。
- fixture 在测试里生成或提交自制的小文件。商业游戏数据、正文、key、存档都不能进仓库。
- 不要写固定措辞、常量、路径拼写或纯转发的测试。

## 9. 本地构建与打包

先初始化子模块（`git submodule update --init --recursive`），安装构建工具。Windows 用 x64 Visual Studio 开发者环境，Android 用 `cargo install cargo-ndk --locked --version 4.1.2` 和 NDK 30.0.16248370。打包前运行 `cargo install cargo-about --locked --version 0.9.3`。完整前提见 [Build and release](../../../docs/en/build-and-release.md#inputs-and-prerequisites)。

```sh
cargo xtask matrix
cargo build --locked --release --target x86_64-pc-windows-msvc -p shinki-emu-core-shinki-emu-<family>
cargo ndk -t arm64-v8a -P 28 build --locked --release -p shinki-emu-core-shinki-emu-<family>
cargo xtask package --target <triple> --core shinki.emu.<family> --version 0.1.0 --out <新目录>
cargo test --locked -p shinki-emu-<family>
```

只构建和打包清单中该核心声明支持的目标。`package` 自行运行 release 构建；`<triple>` 取 `x86_64-pc-windows-msvc` 或 `aarch64-linux-android`。Windows 核心包包含 DLL，Android 独立核心包包含 AArch64 ELF 和 NDK notice，不是 APK。Android 独立构建成功不等于可安装，还需要 `android_dynamic` 策略和设备验收。要构建桌面 Manager，运行 `cargo build --locked --release -p shinki-emu-manager`。

示例用 `cargo build --locked --release -p shinki-emu-tilefix --features dynamic-plugin-export` 构建共享库，用 `cargo test --locked -p shinki-emu-tilefix` 跑宿主测试。要 headless 回放，直接把共享库路径传给 `--core`（§11），无需注册目录或打包。

## 10. 文档、catalog、发布

- 新增 `docs/{zh-cn,en,ja}/cores/<slug>.md`，参照 `docs/en/cores/shinki-emu-cmvs.md`；在各语言 `index.md` 里链接，并在 `README.md`、`README.zh-CN.md`、`README.ja.md` 的引擎表加一行。
- tag `core-<slug>-v<semver>`，推 tag 触发 `release.yml`；发布前在 exact head 上跑通 CI。发布由发布 owner 执行（流程见 `shinkiemu-maintenance` §10），xtask 公开后会自己开 catalog PR。不要给未公开的构建手写 catalog 条目。
- 改了 `Metadata/` 就跑 `cargo xtask metadata validate`。

## 11. PR 门禁与发布验收

**PR 门禁只有构建和打包。** 核心在 Windows x64 和 Android ARM64 中声明支持的目标都跑通 §9 的 Cargo release 构建和 `package`。PR CI 再跑 tools 任务和受影响组件的 Windows/Linux 行。PR 描述写明跑了哪些目标、结果如何。

可选检查，跑了就在 PR 描述里写命令和结果（退出码、帧数或 hash）。截图、日志、hash 和运行输出不进仓库：

- `cargo test --locked -p shinki-emu-<family>`。
- ABI 探针：用 `Source/FamilyApi/shinki-emu-abi/include/shinki_emu.h` 编译 `Tools/krkr-query-probe.c`（名字带 krkr，但对任何核心通用），参数是打包后的库路径；退出码 0 表示 ABI 3 返回完整表、其他版本返回 `UNSUPPORTED` 且输出为空、空输出指针返回 `INVALID_ARGUMENT`。
- headless：`shinki-emu-headless run --core <打包输出里的核心目录> --game-dir <自造或合成的游戏目录> --script <回放> --output <新目录>`（构建用 `cargo build --locked -p shinki-emu-headless --no-default-features`）。示例核心带一份 `Source/Examples/shinki-emu-tilefix/replay.json`。

**发布前由维护者完成**，不是贡献者的 PR 门禁：

1. 所有 supported 目标在同一个 exact-head commit 上 `ci.yml` 成功，blocked 目标有具体原因。
2. ABI 探针在干净进程里通过。
3. 确认公开标签指向产品源码，包内含 `cargo-about` 生成的 Rust 许可及原生依赖声明。
4. 维护者用自己合法持有的游戏，以 headless runner 打开并推进到确有画面输出，结果写进发布记录（用法见 `shinkiemu-maintenance` 的 headless 一节）。存档写在临时副本里。

## 完成检查

- [ ] descriptor、`build.rs`、`cores.toml` 里的 plugin_id 一致，slug 唯一。
- [ ] 每个导出都在 `boundary` 里；`query` 不建状态；`close` join worker。
- [ ] 帧只在变化时递增 `generation`；借出的租约不阻塞 VM。
- [ ] 游戏目录写入都在 `<game>/.shinki/` 下（原格式存档除外）。
- [ ] `cores.toml` 有 `family_id`，覆盖每个 target，`supported_targets` 只列真实构建过的目标；许可与 `build.rs` 一致。
- [ ] 新 crate 的 `Cargo.lock` 变化只包含它引入的条目，没有用 `cargo update`。
- [ ] `frame` 的每个返回值（含 Pending）带真实 session/output epoch，测试里过了 `validate_cpu`。
- [ ] 静态嵌入在 `cores.toml`、Manager `Cargo.toml`、`STATIC_CORES` 三处一致。
- [ ] 行为测试用合成 fixture，`cargo test --locked -p shinki-emu-<family>` 通过。
- [ ] 三语文档和 README 表已更新；`cargo xtask metadata validate` 通过。
- [ ] §11 的 PR 门禁已跑通，可选检查的结果写在 PR 描述里。
- [ ] probe 只按引擎特征识别，没有 EXE 名、Steam appid、固定 hash、固定游戏 ID 白名单；probe 和 open 的接受条件一致。
- [ ] 核心里没有作品枚举，也没有按作品名分派的数据或资源名。
- [ ] 按作品变化的数据按"原版文件解析 → Metadata 按发行版本配置 → Luau 补丁修正非标准源文件"的顺序取得。
- [ ] 存档兼容原版；只在确实无法兼容时隔离，且隔离后真实持久化。
- [ ] probe/open 用至少两款不同作品，或模拟不同归档集合、入口名、目录名、EXE 名的合成 fixture 验证过。
