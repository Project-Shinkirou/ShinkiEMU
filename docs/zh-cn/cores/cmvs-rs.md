---
title: cmvs-rs
description: 外部 CMVS/CVNS 核心的上游覆盖范围、配置和宿主限制。
---

# cmvs-rs

`plugin_id: cmvs-rs` · 引擎家族：CMVS / CVNS

这个外部核心通过[组织 fork](https://github.com/Project-Shinkirou/cmvs-rs/tree/shinki)接入 [Shizuku-in/cmvs-rs](https://github.com/Shizuku-in/cmvs-rs)。它与 ShinkiEMU 自研的 [`shinki.emu.cmvs`](shinki-emu-cmvs.md) 并存，选择其中一个不会替换或配置另一个。两者的 VM、渲染器、兼容范围和存档策略各自独立。

当前暂停发布，真实游戏出帧验收缺少符合上游登记的本地发行版。HAPYMAHER_RE 的启动脚本 hash 不同，HAPYMAHERFD_RE 没有上游登记条目。不修改登记或覆盖 profile 来绕过这项限制。

## 覆盖范围

仅支持上游登记的 CMVS/CVNS 作品。具体发行版本、格式分支和运行限制见[上游登记清单](https://github.com/Shizuku-in/cmvs-rs/blob/b9fed68ba05f80444338331bb3999f14c8f4f976/crates/cmvs-core/profiles/games.txt)和[上游状态报告](https://github.com/Shizuku-in/cmvs-rs/blob/b9fed68ba05f80444338331bb3999f14c8f4f976/docs/status.md)。PS2A 格式探测命中只表示找到了引擎数据，不代表对应发行版本受支持或可玩。未登记的数据或可执行文件会被上游在 open 时拒绝；adapter 保留这项检查，不另加作品白名单。

未登记发行版 open 返回 ABI `NO_MATCH`（8）。只有上游的 `SelectionError::UnknownGame` 和 `UnknownBinary` 做这项状态翻译，拒绝条件与错误消息不变。初次调查中 `UnknownGame` 曾以 status 6（`game data matches no recorded game`）返回，现已正确归类为正常不匹配；IO、缺失资源与解码失败仍为 `CORE_ERROR`。真实 open、截图与帧 hash 验收按用户决定标为“待样本”。

worker 使用上游 `Startup`、CPU/字体场景合成、CMV/MGV 视频解码、原生 Vorbis 音频和快照/存档实现。帧租约持有不可变的完整引擎画布，close 后仍可读取和归还。动态构建导出 C ABI v3 query；静态构建提供相同的 Rust 路径 query，不导出冲突的未修饰符号。

## 配置与存档

- `encoding`：上游的 `auto`、`shift-jis`、`gbk` 或 `utf-8`；自动识别有歧义时明确报错。
- `profile`：可选的上游引擎 profile 覆盖，供研究使用；留空时按上游规则识别。
- `font` / `font-face`：用户提供的 OpenType/TTC 字体文件和面索引。上游字体渲染器要求 Shift-JIS；字体留空时保留上游文字输出屏障。不随核心分发字体。
- `user-data-dir`：上游可选用户数据目录，必须在游戏目录外，默认留空。值原样传给上游，不增加托管存档根，也不重定向到 `.shinki`。不配置时保留上游写档屏障。adapter 不迁移或覆盖既有存档。
- `block-unreviewed` / `block-presumed`：上游命令闸门，默认均为 false。放行命令不等于确认兼容。

## 宿主 ABI 限制

C ABI v3 没有自定义光标位图、显示器枚举/选择或画面比例/滤镜策略字段。纯外观光标请求保留 Host 默认光标，并输出一次诊断；其他无法表达的请求返回 `UNSUPPORTED`，错误说明缺失能力。对话框走宿主窗口服务并支持取消；headless 宿主不会编造用户选择。核心不创建原生游戏窗口。

存档、key、配置的位置和格式沿用上游。二进制与对应源码包保留原 MIT/Apache-2.0 许可证、第三方 notice 和修改声明。精确 pin 与逐目标构建限制见 [`build/cores.toml`](../../../build/cores.toml)。
