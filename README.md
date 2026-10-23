# ShinkiEMU

ShinkiEMU 由 Manager 和原生核心组成，用来运行旧视觉小说引擎的游戏。Manager 管理游戏库、核心与元数据；各核心保留原生 VM、渲染、音频和存档行为。创作引擎另见 [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine)。

ShinkiEMU combines a library Manager with native cores for legacy visual novel engines. Cores own their native VM, rendering, audio and saves. [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) is the separate creation engine.

## 安装 / Install

从 [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) 下载匹配平台的完整 Manager 包，解压后启动 `shinki-emu-manager`（Windows 为 `.exe`）。在设置中添加游戏目录，再从核心目录安装或更新所需核心。只使用实际提供的产物；当前构建与运行报告见[门户](https://project-shinkirou.github.io/zh-cn/shinkiemu/)。

Download the complete matching Manager bundle from [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases), extract it and start `shinki-emu-manager` (`.exe` on Windows). Add game directories in Settings, then install/update cores from the catalog. See the [portal](https://project-shinkirou.github.io/en/shinkiemu/) for real artifacts and reports.

## 开发入口 / Developer entry points

Clone with recursive submodules. Run from this repository's root:

```sh
cargo xtask --help
cargo xtask matrix --json
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
```

- 文档：[中文](docs/zh-cn/index.md) / [English](docs/en/index.md)
- 构建与发布：[中文](docs/zh-cn/build-and-release.md) / [English](docs/en/build-and-release.md)
- [贡献 / Contributing](CONTRIBUTING.md)
- [来源 / Origins](ORIGINS.md) · [第三方声明 / Third-party notices](THIRD_PARTY_NOTICES.md)

自有代码采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可，第三方核心、依赖与素材保留各自许可。

Project-owned code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE). Third-party cores, dependencies and assets retain their own licenses.
