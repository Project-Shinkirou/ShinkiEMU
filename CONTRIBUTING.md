# Contributing / 贡献

## 分支与 PR

从 `main` 建立功能分支，提交聚焦的普通 PR，说明行为变化、复现方式和实际运行的检查。源码、调用方、有效行为回归和文档一起更新。外部核心修改提交到对应组织 fork 的 `shinki` 分支，通过普通 PR 审查；ShinkiEMU 随后更新精确 gitlink，不复制第二份适配源码，也不改上游默认分支。

Build and release entry points are `cargo xtask` commands from the repository root. Work on a topic branch from `main` and open a normal, focused PR describing behavior, reproduction and checks actually run. Update callers, behavior regressions and docs with the implementation. External-core changes belong in the corresponding fork's `shinki` branch through a normal PR; this repository then updates the exact gitlink.

## 检查与测试 / Checks and tests

按修改选择 Manager 或单个核心，避免混合独立核心的依赖图：

Select the component you changed rather than combining independent native dependency graphs:

```sh
cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
cargo xtask metadata validate
```

`test` 无选择时检查 xtask 和构建辅助库；原生测试可能需要真实 GPU/音频和公开夹具。目标与依赖以 `cargo xtask matrix --json` 和 `doctor` 为准，完整入口见[构建文档](docs/zh-cn/build-and-release.md)。

Unselected `test` runs xtask/build-helper tests. Native regressions may require real GPU/audio and public fixtures. Use `matrix --json` and `doctor` for target/prerequisite information; see [Build and release](docs/en/build-and-release.md).

保留格式边界、损坏/超限输入、存档保护、取消、背压、迟到结果、线程退出、CPU 租约和真实消费者回归。只固定文案、常量、路径拼写、参数转发或 mock 调用的测试不保留。不要删除失败的行为回归或关闭真实后端来通过检查。

Keep behavior regressions for format/budget boundaries, corrupt inputs, save integrity, cancellation/backpressure, stale results, worker shutdown, CPU leases and real consumers. Remove tests that only pin wording, constants, path spelling, forwarding or mock calls. Do not delete failing behavior tests or disable real backends to pass checks.

## 来源、许可与敏感数据 / Sources, licensing and sensitive data

保留作者、上游来源和完整许可；引入依赖或可分发附件前核实分发条件。不得提交商业游戏资源、正文、密钥、私有配置、商业存档、解码导出或含敏感数据的日志。回归使用公开授权或自造夹具。兼容性与补丁通过 issue 表单和维护者审查贡献，规则见 [Metadata](docs/zh-cn/metadata.md)。

Retain author attribution, upstream provenance and complete licenses. Check distribution rights before adding dependencies or attachments. Never commit commercial assets/text/saves, keys, private profiles, decoded exports or sensitive logs. Use public licensed or self-created fixtures. Compatibility and patch contributions use issue forms and maintainer review; see [Metadata](docs/en/metadata.md).
