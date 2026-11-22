## 变更 / Change

<!-- 行为变化、原因和复现方式 / Behavior change, reason and reproduction -->

## 构建与打包 / Build and package

<!-- 必填：改动组件声明支持的目标。没有声明支持的写 cores.toml 里的 blocked 原因。
     Required for the changed component's supported targets; for unsupported ones quote the blocked reason from build/cores.toml. -->

- [ ] `x86_64-pc-windows-msvc`: `deps`, `build --release`, `package` (VS x64 developer shell)
- [ ] `aarch64-linux-android`: `deps`, `build --release`, `package`

## 可选检查 / Optional checks

<!-- 跑过才写，每项一两行：命令、退出码、帧数或 hash。截图、日志、hash 和回执不提交进仓库。
     Only what you ran, one or two lines each: command, exit code, frame count or hash. Do not commit screenshots, logs, hashes or receipts. -->

## 来源与许可 / Sources and licenses

<!-- 新依赖、外部核心 pin（组织 fork 提交、upstream_identity）、许可变化；没有就删掉本节。
     New dependencies, external-core pins (organization fork commit, upstream_identity) and license changes; delete this section if none. -->
