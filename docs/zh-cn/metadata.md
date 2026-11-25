---
title: 元数据与补丁贡献
description: 如何提交游戏兼容性报告与 Luau 补丁，以及维护者审核与快照构建流程。
---

# 元数据与补丁贡献

`Metadata/` 是官方门户与 ShinkiEMU Manager 共享的唯一数据源，统一托管在 `https://project-shinkirou.github.io/data/shinkiemu/v1/`。社区可以通过 Issue 表单安全贡献兼容性报告与 Luau 补丁。

## 一、提交游戏兼容性报告

前往[游戏兼容性报告表单](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)，只需提供你的真实运行观察：

1. **查出 VNDB 编号**：在 VNDB 作品页网址中找到 `vID`（如 `v17`），在对应发行版页面找到 `rID`（如 `r1234`）。
2. **填写精确版本与环境**：在 Manager 的**设置 → 关于**与**核心**页查看精确的 Manager 版本、核心 ID 与核心版本，并选择测试使用的平台、架构（`x86_64` / `aarch64`）及变体（桌面选 `desktop`，移动选 `device` / `simulator`）。
3. **选择运行状态**：

| 状态 | 含义 |
|---|---|
| `perfect` | 音画、视频、脚本与存读档全程表现正常 |
| `completable` | 存在轻微瑕疵，但可以正常推完主线 |
| `flawed` | 可进入游戏，但存在明显影响体验的缺陷 |
| `boot_only` | 仅能启动到标题或菜单，无法正常游玩 |
| `unplayable` | 无法启动或立即崩溃 |

> [!CAUTION]
> 请勿在报告中附带任何商业游戏素材、剧本正文、解密密钥或存档文件。仅能编译通过或识别引擎不作为兼容性结论。

每条报告按完整 5 元组 `(release_id, emulator, platform, architecture, variant)` 独立建档，存储于 `Metadata/compatibility/entries/<vid>/<rid>/<plugin_id>/<platform>-<architecture>-<variant>.json`。

## 二、编写与投稿 Luau 补丁

前往[Luau 补丁投稿表单](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)提交补丁配方，详见仓库内的 [shinkiemu-patch](../../.agents/skills/shinkiemu-patch/SKILL.md) 指南。

### 必填要素

- **基础信息**：稳定 ASCII `id`、语义版本号、`title` 与 `summary`（必填 `zh-CN` 与 `en`，可选 `ja`）、精确核心 ID、来源链接、许可证及适用时的授权依据链接。
- **发行指纹绑定（`releases`）**：每条记录绑定具体 VNDB `release_id`、`vn_id`、发行标题、`edition`、游戏版本及源文件指纹列表（`path`、`size`、`sha256`）。Manager 在创建可写事务前会逐项校验指纹。
- **显式下载声明（`downloads`）**：若脚本调用 `ctx.download(url, sha256, size)`，必须在 `downloads` 中声明每一个允许的 HTTPS URL、64 位小写 SHA-256 及字节数（1 B ~ 1 GiB）；空列表禁止下载。跳转须保持 HTTPS，缓存于 `<game>/.shinki/patch/` 并在复用前重新校验。
- **沙箱边界**：脚本运行在统一宿主沙箱中，禁止启动进程、任意联网、路径逃逸、符号链接以及新增/修改 EXE。自动检查仅验证字段与 Luau 语法，不执行脚本也不访问外部 URL。

> [!NOTE]
> 仅填写许可证名称不能代替分发授权证明；涉及第三方汉化文本或素材时请提供明确授权依据。

## 三、维护者审查与快照架构

### Intake 审查流

1. 具有 `write` / `maintain` / `admin` 权限的维护者审查 Issue 后，添加 `compat:accepted` 或 `patch:accepted` 标签；
2. 自动化工作流将 Issue 正文作为纯数据处理，记录原文 SHA-256 与 `updated_at`，通过普通 PR 生成键控源记录与 `Metadata/intake/<issue>.json`（不执行脚本）；
3. 若 Issue 正文在批准后被编辑，工作流会自动撤销批准标签并关闭候选 PR，要求重新审查。

### 核心发行目录自动化

每次核心发布后，自动化工作流会为对应平台生成 `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json` 并提交 PR；受信任的校验工作流验证通过后自动合并，门户站定时构建随后发布最新快照。

### 快照格式与版本兼容

- **核心目录**：快照同时发布 `cores-v2.json`（`shinki.emu.cores.v2`，含第三方 `identity` 身份字段）与兼容旧 Manager 的 `cores.json`（v1）；新版本优先读取 v2。
- **兼容性索引**：快照同时发布 `compatibility-v5.json`（核心版本上限 88 字节，适配第三方完整标识）与旧版 `compatibility.json`（v4，上限 64 字节）。
- **补丁目录**：`patches-v2.json`（`shinki.emu.patches.v2`）汇总 `Metadata/patches/entries/<id>.json` 与已审查 `.luau` 脚本；旧 Manager 继续读取从 `Metadata/legacy` 生成的 v1 `patches.json` 与 `patches/catalog.json`。
- **确定性快照与限额**：`index.json` 记录全部索引文件的 SHA-256、大小与源提交哈希（单文件 ≤ 8 MiB、总计 ≤ 64 MiB、≤ 4096 个文件）。

### 常用校验与构建命令

```sh
cargo xtask metadata schema
cargo xtask metadata validate
cargo xtask metadata build --root Metadata --out .work/metadata-snapshot
cargo xtask metadata resolve-core --plugin-id rfvp --platform windows-x86_64 --abi shinki.emu.native.c.v3
cargo xtask metadata intake --kind compatibility --event event.json --out .work/compat-candidate
cargo xtask metadata intake --kind patch --event event.json --out .work/patch-candidate
```

Schema 由 Rust DTO 自动导出至 `Metadata/schema/`，请勿手工维护第二份 schema。
