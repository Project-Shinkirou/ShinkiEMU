---
name: shinkiemu-maintenance
description: Use when maintaining ShinkiEMU itself rather than writing a core - Cargo builds and xtask packaging and releases, build/cores.toml and targets.toml edits, external-core version identity (upstream_identity, release_sequence, canonical core tags), fork pin bumps and Cargo.lock discipline, vcpkg baseline/overlay triplets/overlay ports, Android/iOS static Manager embedding, license notices and public fork commits, GitHub CI exact-head runs and watchers, the contributor PR gate versus maintainer release acceptance, release/catalog publication, signing (Android release certificate, Apple ad-hoc, OHOS test PKI), native macOS builds and Mac disk capacity, and headless runs (shinki-emu-headless run/compare/ui, shinki-emu-manager headless/headless-ui) for frame acceptance, screenshot and hash baselines, input replay scripts, null audio selection and windowless Manager UI screenshots. Not for writing a new core (shinkiemu-new-core), porting a third-party engine (shinkiemu-integrate-existing-core) or Luau game patches (shinkiemu-patch).
---

# ShinkiEMU 项目维护

本 skill 记录维护 ShinkiEMU 时的实际流程和踩过的坑。契约以仓库文档为准，这里只写操作顺序、检查点和容易出错的地方：

- [Build and release](../../../docs/en/build-and-release.md)（中文 `docs/zh-cn/`，日文 `docs/ja/`）
- [Core ABI](../../../docs/en/core-abi.md)、[Metadata](../../../docs/en/metadata.md)
- [xtask/README](../../../xtask/README.md)、[ThirdParty/README](../../../ThirdParty/README.md)

写新核心看 `shinkiemu-new-core`，移植第三方引擎看 `shinkiemu-integrate-existing-core`，写或审核 Luau 游戏补丁看 `shinkiemu-patch`。

审查核心或 adapter 的代码改动时，检查 probe、open、资源、存档和标题/鉴赏路径里有没有作品白名单或作品枚举（EXE 名、Steam appid、固定 hash、固定游戏 ID、写死的作品资源名或入口），有就退回，按 [shinkiemu-new-core](../shinkiemu-new-core/SKILL.md#引擎适配器不是作品播放器) 和 [shinkiemu-integrate-existing-core](../shinkiemu-integrate-existing-core/SKILL.md#引擎适配器不是作品播放器) 的规则改成引擎特征识别，作品数据改走原版文件解析、Metadata 发行版本配置或 Luau 补丁。

## 1. 权威文件

| 文件 | 管什么 |
|---|---|
| `build/cores.toml` | 核心 ID、引擎族、许可、`supported_targets`/`[[cores.blocked]]`、`android_dynamic`、`ios_framework`、`publish_blocked` 和外部核心的 `[cores.attribution]` |
| `build/targets.toml` | 10 个 triple、runner、`manager_supported`；target 级 `blocked` 覆盖所有组件 |
| `build/dependencies.toml` | 工具版本和下载输入（SHA256 固定） |
| `build/vcpkg/` | `ffmpeg/`、`subtitle/` manifest；`apple-triplets/`（arm64/x64-osx、arm64-ios、arm64-ios-simulator、x64-ios）；`mobile-triplets/`（`*-android-ffmpeg`、`arm64-ohos`、`ohos-toolchain.cmake`） |
| `Source/Manager/shinki-emu-manager/Cargo.toml` | Manager feature；已弃用的 `static-cmvs` 和 `static-adv32` 仅供本地兼容构建 |
| `Source/Manager/shinki-emu-manager/src/static_cores.rs` | 兼容构建的 `STATIC_CORES` 入口表；默认构建为空 |
| 根 `Cargo.toml` + `Cargo.lock` | 核心 fork Git URL/提交和整个工作区的依赖；根 `[patch]` 将 fork 使用的 ShinkiEMU crate 指向树内实现 |
| `Source/Cores/<id>/` | 单独构建的核心入口；按需开启实现的独立导出 feature |

改清单后用 `cargo xtask matrix` 检查目标。每个正式目标 triple 在 `supported_targets` 或 `[[cores.blocked]]` 中恰好出现一次；slug 不能重复，外部 plugin slug 不超过 32 字节。

文档改动要三语同步：`docs/{zh-cn,en,ja}/` 同名文件一起改。

## 2. 工作区与隔离

多个 agent 或多人并行工作时，每个 slice 用自己的 worktree 和输出目录，避免互相踩 Cargo lock 和 native 缓存。路径按本机磁盘选择，下面是形状，不是固定位置：

```text
worktree               <worktrees>/<slice>                  （产品）
fork worktree          <worktrees>/forks/<fork>-<slice>
CARGO_HOME             <cargo-home>                         （可共享）
CARGO_TARGET_DIR       <targets>/<slice>
SHINKI_EMU_TARGET_ROOT <短路径根>/<slice>                    （Windows 短路径根）
TEMP/TMP               <tmp>/<slice>
证据与发布实物          <release-scratch>/<owner>/             （不进仓库）
```

- 新 worktree 从最新公开 `origin/main` 开：`git worktree add -b <branch> <worktrees>/<slice> origin/main`，再 `git -C <worktrees>/<slice> submodule update --init --recursive` 取得 `ThirdParty/cpal`。
- Cargo 根据根 `Cargo.toml` 和 `Cargo.lock` 获取 fork 依赖。要用自己的 fork 工作树调试，在根 `Cargo.toml` 添加临时 `[patch]`，运行 `cargo metadata --format-version 1` 重新解析后构建。提交前移除 patch，恢复公开 pin。示例见 [Build and release](../../../docs/en/build-and-release.md#work-on-a-fork-locally)。
- 同一时刻 Mac 只有一个 native slot，`main`、tag、catalog 只有一个发布 owner。没被指派时不要 push `main`、打 tag 或开 catalog PR。

## 3. 构建命令

```sh
cargo xtask matrix                      # 所有组合及 blocked 原因
cargo build --locked --release --target <triple> -p <package>
cargo check --locked --target <triple> -p <package>
cargo test --locked -p <package>
cargo ndk -t arm64-v8a -P 28 build --locked --release -p shinki-emu-manager --lib
cargo xtask package --target <triple> --core <id> --out <新目录>      # 外部核心不传 --version
cargo xtask package --target <triple> --manager --version <semver> --out <新目录>
cargo xtask ci --target <triple> --component <id|manager> --report .work/<name>.json
cargo xtask metadata validate
```

- Manager 包名 `shinki-emu-manager`。独立核心入口包名为 `shinki-emu-core-<id>`，把 ID 中的点换成连字符，保留大小写与下划线。测试直接选实现 crate。
- Cargo build script 准备原生依赖。`package` 运行锁定依赖的 release 构建，从 Cargo JSON 读取实际产物和打包输入，输出必须是新目录。打包、`ci` 和 release 准备前安装 `cargo install cargo-about --locked --version 0.9.3`。自研核心和 Manager 打包必须传产品 `--version`。
- 本地 Cargo 用 `CARGO_TARGET_DIR`，Windows 路径过长时将它设为短路径。xtask 启动的构建也接受此变量，或用 `SHINKI_EMU_TARGET_ROOT` 调整短路径根。
- 构建按组件选择包，不做 workspace-wide all-features 构建。发布组件统一使用根工作区的锁文件。
- CI 的 tools job 跑的是 `cargo fmt --check`、`cargo clippy --locked -p xtask -p shinki-emu-build --all-targets -- -D warnings`、`cargo test --locked -p xtask -p shinki-emu-build`、`cargo xtask metadata validate`，以及用 cbindgen 0.29.2 比对 `Source/FamilyApi/shinki-emu-abi/include/shinki_emu.h`。本地改 xtask 后跑同一组即可；改了 `shinki-emu-abi/src` 就按 `docs/en/core-abi.md` 的命令重新生成头文件一起提交。
- Windows 的 Cargo 和打包命令在 x64 VS 开发者环境里跑。打包从 `%VCToolsRedistDir%\x64` 取 MSVC 运行库。
- 新增 workspace 成员和依赖时，不带 `--locked` 跑一次 `cargo metadata --format-version 1` 更新锁文件，核对预期差异后用 `--locked`。fork pin 升级见 §5。

**贡献者 PR 门禁与维护者职责。** 外部贡献者在 Windows x64 和 Android ARM64 中为改动组件声明支持的目标上跑通 Cargo release 构建和 `package`。PR CI 再跑 host 工具检查和受影响的 Windows/Linux 行。测试、ABI 探针、headless 和示例运行的结果写在 PR 描述里，截图、日志、hash 和运行产物不进仓库。§11 的发布验收、macOS/iOS/OHOS 构建、Android Manager 双 ABI 打包和组织 fork 写入由维护者负责。

平台优先级与顺序：Windows x64 → Linux x64 → macOS（arm64、x64）→ Android（独立 ZIP）→ iOS（签名 Framework）→ OHOS（DevEco 本地打包）。

## 4. 外部核心版本身份

外部核心不用 SemVer。`Cargo.lock` 提供组织 fork 的仓库和完整提交；清单中另有：

```toml
[cores.attribution]
adapter_branch = "<fork 的真实分支>"
release_sequence = 1
upstream_identity = { repo = "https://github.com/<原作者>/<repo>", ref_kind = "tag", ref_name = "<原上游 tag>", commit = "<原上游完整 commit>" }
```

- `upstream_identity` 记录原上游。上游 commit 有 tag 时用 `ref_kind = "tag"`；否则用包含该 commit 的真实上游分支。组织 fork 的分支只用于 `adapter_branch`。
- digest 是 8 个字段（upstream repo/kind/ref/commit、adapter repo/branch/commit、十进制 sequence）各接一个 LF 后的完整 SHA256。版本为 `<tag|branch>-<ref slug>-<digest>`，tag 为 `core-<plugin slug>-<version>`。只改 adapter 也会得到新身份。
- `release_sequence` 每个插件全局递增，跨平台共用一个序号。adapter/upstream pin 变了或要重新发布时 +1；同一序号不能对应两个不同版本，也不能回退。多个候选并行时，序号由发布 owner 统一预留，slice 不要自己占号。
- 改完跑 `cargo xtask release --component <id> --dry-run`，核对版本、标签和目标。这个命令不构建、不发布。
- Manager 和自研核心用 `manager-v<semver>` / `core-<slug>-v<semver>`，`package`/`release` 必须传 `--version`。外部核心省略 `--version`。
- 已公开的 tag 和资产不可改写。修复只能走新 sequence 的新 release。

## 5. fork pin 升级

1. fork 改动在 fork 分支提交并 push（非 force），取完整 SHA。
2. 在根 `Cargo.toml` 的 `[workspace.dependencies]` 更新 `rev`，入口和 Manager 继续继承同一个依赖。需要时更新 `build/cores.toml` 的 `adapter_branch`、`upstream_identity` 和 `release_sequence`。
3. 跑 `cargo metadata --format-version 1` 更新 `Cargo.lock`。检查变更限于这次 fork 升级所需的提交和依赖，不批量更新无关依赖（例如不要改变 D3D12 所用的 `windows` 版本）。
4. 用 `cargo metadata --locked --format-version 1` 和 `cargo xtask matrix` 检查锁文件与目标清单；按受影响目标重建。
5. Android 和 iOS 的静态核心共享 Manager 的依赖图：kira、cpal、mlua/mlua-sys、wgpu、naga 必须能统一到一个版本。两个版本的 `links =` crate 会让解析失败。优先改 fork 对齐，不要再加 patch。iOS 需要同时开 naga 26 的 `termcolor` 特性，原因见 Manager `Cargo.toml` 注释。

同一 fork 的多条平台线（例如 Linux、Mac、Android 各自修过 kirikiroid2-web）要用真实 merge 合到一个共同分支，保留每条线的祖先。不要 squash，不要 rebase 已推送的分支。合流后必须在共同 head 重跑全部目标的门禁，旧分支的通过记录不能挪用。

## 6. vcpkg 与原生依赖

- 以 `build/dependencies.toml`、各 vcpkg manifest 和 overlay 为准。构建脚本从 `.work/vcpkg` 引导固定 vcpkg，或用 `SHINKI_EMU_VCPKG_ROOT` 指向已固定的共享 checkout。
- 缓存是 `.work/vcpkg-binary-cache/` 和 `.work/vcpkg-downloads/`。下载不稳定时可以预放同版本源码包，vcpkg 仍按 port 的 SHA512 校验。
- 安装树在 `.work/vcpkg-installs/<triplet>-<key>/installed/`，各架构和清单互不覆盖。FFmpeg 和 libass 由共享 `links` crate 供直接消费者使用。
- Apple overlay triplet 固定 macOS 13.0/iOS 16.0，并让 libass 用 CoreText。原生工具在宿主运行，SDK 和交叉环境由调用方提供。
- 修原生依赖时改 overlay port 或 patch（例如 kirikiroid2-web fork 的 `vcpkg/ports/<port>/`），保持配置校验开启。AppleIos64 的经验：
  - GLib 工具链接缺 CoreFoundation：给 `libintl.patch` 加真实的 Meson `appleframeworks` 依赖，而不是手塞链接参数。
  - libgdiplus 在 iOS 编到 Carbon：用 OSX 宏把 Carbon 代码限定在 macOS，iOS 走原有的 Cairo/Pango 路径。
  - OpenCV 在 AppleClang 上找不到 OpenMP：新增只构建 CPU libomp 的 `llvm-openmp` overlay port（官方 `openmp-18.1.6.src` + `cmake-18.1.6.src`），由真实 FindOpenMP 发现。不要强设 `OpenMP_FOUND`、伪造 probe 结果或关掉 `ENABLE_CONFIG_VERIFICATION`。
  - Autoconf 在 iOS 上把 host/build 判成相同，会去执行 iOS conftest。用 triplet 修正 host 三元组，不要恢复强制 probe cache。
- 构建失败时先读真实日志里的第一个错误，修完再跑下一轮。一轮只修一个首错，修复提交后再开新 run。

## 7. 移动端核心打包（Android / iOS / OHOS）

默认和正式 Manager 按独立二进制加载核心：Android 安装符合 `android_dynamic` 策略的核心 ZIP；iOS 从应用包加载各自签名的 Framework；OHOS 从签名 HAP 加载独立的 Musica 动态库。iOS 和 OHOS 不接受用户安装可执行核心。

已弃用的 `static-cmvs` 和 `static-adv32` 仅保留本地兼容入口。构建会提示改用独立核心包，`cargo xtask release` 拒绝这两个 feature。

Android 发布签名仍须符合以下要求：

- Android 正式安装包必须用发布证书签名，证书 SHA256 为 `c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc`。签名材料从 `SHINKI_EMU_ANDROID_KEYSTORE`（或 `_KEYSTORE_BASE64`）、`_KEYSTORE_PASSWORD`、`_KEY_ALIAS`、`_KEY_PASSWORD` 读取，CI 从 secrets 注入。没有这些时产物是 `*-debug-signed.apk`，只能当 CI 测试包。
- `release.yml` 用 `github.run_number` 设置 `SHINKI_EMU_ANDROID_VERSION_CODE`。不要重建、改名或替换工作流文件，否则编号可能从 1 重来，已安装的用户无法升级。`package` 核对正式签名证书、包名 `org.shinkirou.shinkiemu`、`versionName`、`versionCode` 和 `minSdk` 28；公开 APK 的 `versionCode` 必须高于上一版。

## 8. 公开源码与许可证

- 每个外部核心的公开源码就是 `Cargo.lock` 记录的公开 fork 提交。发布前确认该提交可获取。
- 保留上游 `LICENSE`、notice 和 `MODIFICATIONS.md`。`build/cores.toml` 用 `adapter_licenses` 指定 fork 内许可，用 `licenses` 指定产品树内的许可。原生依赖保留原声明；Rust 依赖许可由 `cargo-about` 0.9.3 收集。
- FMOD、Spine、MS PGothic 等条款单独记录，不改标为 MIT。按所选许可履行对应源码和重链接义务。
`publish_blocked` 非空的核心可以编译，但二进制不会上传。

## 9. CI 与 watcher

`ci.yml` 在 push（main、`release/**`）、PR 和 `workflow_dispatch` 时运行。dispatch 输入有 `since`（只跑受影响行）、`components`、`targets`。它的 concurrency 是 `ci-${{ github.ref }}` 且 `cancel-in-progress: true`：同一 ref 的新 push 会取消正在跑的 run。

做 exact-head 门禁时：

1. 为要验收的 commit 推一个专用、不再移动的 ref（例如 `candidate/<slice>-<gate>`），在这个 ref 上 dispatch：
   `gh workflow run ci.yml -R Project-Shinkirou/ShinkiEMU --ref <ref> -f components=<id> -f targets=<t1,t2>`
2. 用 `gh run view <run> -R Project-Shinkirou/ShinkiEMU --json headSha,status,conclusion,jobs` 确认 `headSha` 是你要验收的 commit。
3. 每个 run 只挂一个 watcher：`gh run watch <run> -R Project-Shinkirou/ShinkiEMU --exit-status --interval 60`，作为后台任务等它自然结束，不轮询。
4. 失败时读 `gh run view --job <job> -R Project-Shinkirou/ShinkiEMU --log-failed`，找第一个真实错误。
5. 接手别人的 run 时只查一次状态，需要时再挂唯一 watcher。同一源码已经有 run 时不要重复 dispatch。被新 head 取代的 run 先保存日志和 artifact 元数据，经 owner 同意再取消。

`release.yml` 在推 `manager-v*`/`core-*` tag 时运行，也可 dispatch。输入有 `version`（Manager/自研核心）、`components`、`release_scope` 和 `prepare_only`；仅准备时可用 `source_commit` 选择检出。一次推超过三个新 tag 不会触发工作流。

## 10. 发布

发布 owner 在干净 checkout 中按下面顺序操作：

1. 选用同一源码提交上成功的发布运行。全部平台的 Manager 发布不要覆盖 `source_commit`，并用 `gh run view <run> --json headSha --jq .headSha` 对照本地 `git rev-parse HEAD`。
2. 下载该运行的所有目标产物，用 `gh run download <run> -R Project-Shinkirou/ShinkiEMU -p 'release-files-manager@*' -D <scratch>`，再把各产物中的 `<tag>/<target>/` 放到 `.work/release/<tag>/`。
3. 全平台 Manager 要在这份检出上设置好 DevEco 和签名目录，本地准备 HAP，再用授权的 `GH_TOKEN` 或 `GITHUB_TOKEN` 发布：
   ```sh
   cargo xtask release --component manager --target aarch64-unknown-linux-ohos --version <semver>
   cargo xtask release --component manager --version <semver> --scope all --publish
   ```
   外部核心发布用 `cargo xtask release --component <id> --publish`，不传版本。只发 Windows/Android 时，各步骤用同一个 `--scope windows-android`。
4. 核心公开后，xtask 把每个平台的 catalog 条目提交到 `metadata/release-<tag>` 并开 PR 到 main（`Metadata/engines/cores/<plugin_id>/<platform>/<version>.json`）。这个目录 PR 由既有的 GitHub Actions bot 按批准流程自动 squash 合入，这是唯一允许 squash 的地方，不要改它的自动策略。"不 squash、不 rebase"约束的是被 pin 的产品源码和 fork 历史。
5. 公开后匿名 HEAD 每个资产，确认 HTTP 200，并核对 API digest/size 与本地记录一致。不需要重复整包下载。

签名边界要在 release 说明里如实写明：Windows 包未签名；macOS/iOS 用 ad-hoc 签名，iOS 是测试产物；OHOS HAP 用 SDK 的 debug test PKI；Android 必须是 c415 发布证书。

## 11. 验收范围

维护者在发布前检查：支持目标在同一提交构建通过，包正常签名且证书正确，许可证随包，锁定的外部源码可获取，ABI query 探针和 headless 出帧通过（§13）。这些不是贡献者的 PR 门禁（§3）。

除非任务另有要求，不额外重建取证或重复下载大资产。实体设备、模拟器、真实账号上的启动验收另行安排；报告注明实际执行的检查。新核心发布前仍要用维护者合法持有的游戏在桌面 headless 打开并出帧。

## 12. Mac 原生构建与容量

- Mac 原生构建由维护者在自己的 Mac 上执行。主机别名、SSH 包装脚本、驱动脚本和 Mac 上的路径属于维护者本机配置，不写进仓库；下面只写必须遵守的约束。
- 一次只跑一个 native 任务，用一个真实的锁文件串行化。开始前确认锁不存在、没有 cargo/rustc/xtask/vcpkg/clang/cmake 进程。
- 每个 target 行开始前检查剩余空间不少于 12 GiB，不自动删目录。需要 MacPorts 的 `glibtoolize`（设 `LIBTOOLIZE`）和 Bison 3.8.2，系统自带的 Bison 2.3 太旧。
- 结果以每个 target 行自己的 summary 和日志为准。SSH 包装进程退出码 0 不代表构建成功。
- 空间不足 12 GiB 时停下，把实际阶段和剩余字节报给发布 owner。不要删 Cargo target、installed 树或下载缓存来腾空间；清理要有明确授权。
- Mac 上为 xtask 和每个 target 分别使用独立的 `CARGO_TARGET_DIR`、`SHINKI_EMU_TARGET_ROOT`，以及专用的 cargo home 和 rustup 目录，沿用已有的隔离根，不和其他任务共用。

## 13. Headless 验收与调试

headless runner 是 crate `Source/Programs/shinki-emu-headless`。完整契约写在 [Headless execution](../../../docs/en/headless.md)，本节只讲验收时怎么用。它复用现有通用层，没有另建协议：

- 核心加载和会话走 Manager 同一套 `shinki-emu-manager-core`（C ABI v3 loader、原生会话 owner、不可变 CPU 帧租约）。跑核心时不创建 Slint 窗口、显示表面或事件循环。
- 输入脚本沿用 RoukakuEngine `roukaku-headless-protocol` 的 `roukaku.user_input_sequence.v1`，用的是工作区已经 pin 的 Engine revision。

入口：

```sh
shinki-emu-headless run     <参数>      # 动态核心：--core 写 core ZIP、解开的核心目录或库路径
shinki-emu-manager  headless <参数>     # 桌面 Manager：--core 同样写核心包或动态库路径
shinki-emu-headless compare --actual a.png --baseline b.png [--max-channel-delta N --changed-pixel-ratio R]
shinki-emu-headless ui --page library|cores|patches|settings --language zh-CN|en|ja --theme dark|light --output <dir>   # Manager: headless-ui
```

构建独立运行器：`cargo build -p shinki-emu-headless`。加 `--no-default-features` 可省去 Manager 界面。只有显式开启已弃用 feature 的本地 Manager 兼容构建才接受链接核心 ID。

`run` 的主要参数：

| 参数 | 作用 |
|---|---|
| `--core` | 核心 ZIP、解压目录或动态库路径 |
| `--game-dir` | 源游戏目录。核心拿到路径之前，整个目录先复制到唯一的临时目录，存档和相对路径配置都写在副本里。symlink 和特殊文件会被拒绝；大游戏会占磁盘和时间 |
| `--frames`（默认 60）、`--duration-ms` | 时间线长度，取两者和脚本结束点中的最大值 |
| `--capture-frame`、`--capture-ms` | 截图时间点，逗号分隔；采样号从 1 开始，毫秒向上取整到下一个采样。最后一个采样总会截图 |
| `--output` | 输出目录 |
| `--baseline <目录>` | 与目录中同名 PNG 比较；`--max-channel-delta`、`--changed-pixel-ratio` 给容差，都为 0 时精确比较，尺寸必须一致 |
| `--baseline-hash sha256:<hex>` | 断言最后一帧的规范像素 hash |
| `--script <file>` | 输入脚本 |
| `--configuration <file>` | Manager `ConfigEntry` 数组，例如 `[{"id":"profile_path","value":{"String":"key.toml"}}]`，字段 id 用核心实际声明的 |
| `--startup-timeout-ms`（默认 30000） | native open 之后等第一帧完成的上限 |

**时钟的含义。** C ABI v3 下游戏时钟归核心所有，没有确定性的单步推进。runner 从第一帧完成后开始，按 16 666 667 ns 的主机时间线采样。`--frames N` 是 N 个主机采样点，不是 N 个 VM tick。`acquired_frames` 是实际取到的不同帧数；静态标题画面在多个采样点相同是正常的。两次运行逐像素一致取决于核心和游戏自身的时序，不能宣称 native stepping 是确定的；需要时用容差比较。

**输出。** 每个截图是 `<id>.png` 加 `<id>.json`。sidecar 记录尺寸、采样号、`pixel_hash`、`png_hash` 和比较结果。成功时写 `run.json`，失败时写 `failure.json`（诊断信息和退出码）。比较失败也会保留实际 PNG 和比较 sidecar。`pixel_hash` 是规范像素的 SHA256（加 `sha256:` 前缀）：自上而下、紧密排列的 RGBA8 sRGB，alpha 固定 255，BGRA、带 padding 的 stride、自下而上的行序和 linear 格式都先归一化。所以 PNG 编码差异不影响它。

**退出码。** 0 成功；1 运行、输入、文件、ABI 错误或没出帧；2 命令行语法错误（仅独立二进制）；3 基线或回放断言失败。

**脚本。** JSON 数组，每项一条 `InputMessage`：`sequence` 严格递增，`tick` 不减，`session` 不变。指针坐标是归一化的 0..65535。一次点击是 move + press + release，按键的按下和抬起都要显式写：

归一化指针按显示帧的 raster 尺寸映射，核心再转换到 logical 坐标；两者不同时运行器不重复缩放。该修复随 4b119d19 合入，runner 不属于旧 beta.7 下载包。

```json
[
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":1,"tick":0,"event":{"type":"advance_ticks","count":30}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":2,"tick":30,"event":{"type":"pointer_move","x":32768,"y":32768}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":3,"tick":30,"event":{"type":"pointer_button","button":"primary","state":"pressed"}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":4,"tick":31,"event":{"type":"pointer_button","button":"primary","state":"released"}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":5,"tick":40,"event":{"type":"keyboard","physical_key":"Enter","logical_key":"Enter","state":"pressed","repeat":false}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":6,"tick":41,"event":{"type":"keyboard","physical_key":"Enter","logical_key":"Enter","state":"released","repeat":false}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":7,"tick":60,"event":{"type":"await","observation":{"kind":"exists","key":"frame"},"timeout_ticks":60}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":8,"tick":60,"event":{"type":"checkpoint","id":"after-confirm"}}
]
```

- 支持的事件：`resume`、`focus`、`keyboard`、`ime_commit`、`pointer_move`、`pointer_button`、`wheel`、`advance_ticks`、`await`、`checkpoint`。其他协议事件会明确报错。
- 按键名用 Manager 的物理键名（`KeyA`、`Enter`、`ControlLeft`、`ArrowDown`、`F1` 等）。
- `advance_ticks` 推迟后续消息，不影响已排好的截图。
- `await` 只支持 `{"kind":"exists","key":"frame"}` 和 `{"kind":"equals","key":"frame.sha256","value_hash":"sha256:<64 hex>"}`；超过 `timeout_ticks` 未满足即断言失败。`continue_at_match` 必须为 false，因为核心时钟无法回退。
- `checkpoint` 导出 `<id>.png`，并与 `--baseline` 里的同名文件比较。id 是唯一的扁平文件名，`frame-` 前缀留给定时截图。

**常见用法。**

1. **新核心出帧验收**（发布前由维护者完成，见 `shinkiemu-new-core` §11）：`run --core <打包好的核心目录或 ZIP> --game-dir <合法持有的游戏> --frames 600 --capture-frame 120,600 --output <新目录>`。要求 `run.json` 存在、`acquired_frames > 0`，并打开 PNG 确认是游戏画面而不是全黑或纯色。结果写进发布记录，输出不进仓库。贡献者可以用自造或合成的游戏目录跑同样的命令，结果写进 PR 描述即可。
2. **回归对比**：把第一次运行的输出目录作为 `--baseline`。GPU 合成可能有细微差异，给小容差，例如 `--max-channel-delta 4 --changed-pixel-ratio 0.001`。单张图用 `compare`。
3. **流程回放**：在关键步骤后放 `checkpoint`，用 `await` + `frame.sha256` 断言到达已知画面。
4. **Manager UI 截图**：`ui` / `headless-ui` 用真实 `ManagerWindow` 经软件渲染出图，不创建窗口，不启动数据库、库扫描、账号、商店或网络服务。默认数据是自写的合成文本，不含封面、游戏可执行文件或商业截图；`--demo-data demo.json` 只提供显示字符串，不会被扫描。语言用 `zh-CN`、`en`、`ja`，主题用 `dark`、`light`，宽高为物理像素（360..8192）。设置页分类为 `library-roots`、`appearance`、`network`、`translation`、`input`、`about`。不同主题用不同输出目录。
5. **空音频**：核心声明了 `headless_audio_device` 枚举字段且含 `nullaudiodevice` 时，可加 `--audio-device nullaudiodevice` 在没有声卡的机器上验收逻辑和出帧。未声明时 runner 以退出码 1 拒绝；它不证明有声音，也不能当成真实设备失败时的回退。

**限制。** 复制游戏只是隔离普通存储，不是给不可信核心用的系统沙箱。核心运行在隔离子进程中，AppData/HOME/XDG 目录和 TEMP 环境临时化；核心声明的字符串 `save_root` 路径被强制指向临时目录。`RUST_MIN_STACK` 默认至少 8 MiB，保留显式更大值。配置里故意指向游戏外的路径，要改到临时位置。源游戏目录始终只读。核心自己需要的 GPU 或音频设备照样需要；runner 不会关掉音频、影片或脚本。Musica、CMVS 这类硬件 GPU 合成的核心，软件 adapter 的结果不算通过。

**平台状态。** 历史 Windows x64 验收用静态 Siglus 跑过 Rewrite_PLUS，也用动态 RFVP ZIP 跑过樱花萌放。两者各采样 360 次、回放 8 条输入，取得 1280×720 截图和 hash，独立重跑后通过容差比较。独立运行器现用动态包路径；静态结果只作为历史记录。Linux x64 默认构建（含 UI）只做过 `cargo check`。这组验收没有在 macOS、Android、iOS 或 OHOS 上运行。

**调试。** 先用 runner 复现问题，再看核心的 `tracing` 事件（`shinki.emu.<family>.*`）。核心 `SESSION_FAILED` 时，错误码和消息会写进 stderr 和 `failure.json`。Musica 另有只读诊断 CLI `shinki-emu-musica-cli data list|read|validate`（见 `Source/Programs/shinki-emu-musica-cli/README.md`），它不运行 VM。

## 完成检查

- [ ] `cargo xtask matrix` 通过；文档三语同步。
- [ ] 外部核心 `release --dry-run` 打印的 tag 符合预期；序号由发布 owner 确认。
- [ ] `Cargo.lock` 只有预期 pin 和依赖变更，未提交本地 patch，正式 Manager 未启用已弃用的静态核心 feature。
- [ ] 每个支持目标在同一 exact head 上 CI 成功，并且只有一个 watcher。
- [ ] 公共 fork 的锁定提交可获取，许可证随包，签名边界写进发布说明。
- [ ] 公开 tag/资产没有改写；源码和 fork 历史用真实 merge，catalog PR 走既有 bot 自动 squash。
